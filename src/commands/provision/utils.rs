use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::agent_adapters::{SOURCE_BUILT_IN, load_agent_adapters, preferred_discovery_root};
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::helpers::{map_io, shell_arg, validate_non_empty};
use super::super::{App, CommandFailure};

pub(super) struct NormalizedCloneUrl {
    pub display: String,
    pub clone_url: Option<String>,
    pub secret_redacted: bool,
    pub local_only: bool,
}

pub(super) fn resolve_workspace(
    _app: &App,
    workspace: Option<&Path>,
) -> std::result::Result<PathBuf, CommandFailure> {
    let path = match workspace {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => std::env::current_dir().map_err(map_io)?.join(path),
        None => std::env::current_dir().map_err(map_io)?,
    };
    Ok(normalize_existing_or_raw(&path))
}

pub(super) fn validate_provision_agent(agent: &str) -> std::result::Result<(), CommandFailure> {
    validate_non_empty("agent", agent)?;
    if agent.len() > 64
        || !agent
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("agent '{agent}' must match [a-z0-9_-]{{1,64}}"),
        ));
    }
    Ok(())
}

pub(super) fn provision_next_actions(target: &str, workspace: &Path, agent: &str) -> Vec<String> {
    vec![format!(
        "loom provision plan --target {} --agent {} --workspace {}",
        target,
        agent,
        shell_arg(workspace)
    )]
}

pub(super) fn container_workspace_path(workspace: &Path) -> String {
    let name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace");
    format!("/workspaces/{name}")
}

pub(super) fn target_skill_path(container_workspace: &str, relative: &str) -> String {
    if relative.is_empty() {
        return container_workspace.to_string();
    }
    format!(
        "{}/{}",
        container_workspace.trim_end_matches('/'),
        relative.trim_start_matches('/')
    )
}

pub(super) fn target_skill_path_relative(
    ctx: &AppContext,
    workspace: &Path,
    agent: &str,
) -> std::result::Result<String, CommandFailure> {
    let workspace = normalize_existing_or_raw(workspace);
    let adapters = load_agent_adapters(ctx)?;
    if let Some(adapter) = adapters.adapter_for_agent(agent)
        && adapter.has_discovery_root_for_scope("project")
    {
        match preferred_discovery_root(adapter, "project", &workspace) {
            Ok(root) => {
                let root = normalize_path(&root.path);
                let relative = root.strip_prefix(&workspace).map_err(|_| {
                    CommandFailure::new(
                        ErrorCode::AdapterInvalid,
                        format!(
                            "adapter '{}' project discovery root '{}' is outside workspace '{}'",
                            agent,
                            root.display(),
                            workspace.display()
                        ),
                    )
                })?;
                return Ok(path_to_slash(relative));
            }
            Err(_err) if adapter.source == SOURCE_BUILT_IN => {}
            Err(err) => return Err(err),
        }
    }
    Ok(match agent {
        "codex" => ".agents/skills".to_string(),
        other => format!(".{other}/skills"),
    })
}

pub(super) fn path_to_slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

pub(super) fn normalize_existing_or_raw(path: &Path) -> PathBuf {
    let normalized = normalize_path(path);
    if let Ok(canonical) = fs::canonicalize(&normalized) {
        return normalize_path(&canonical);
    }

    let mut probe = normalized.clone();
    let mut suffix = Vec::new();
    while !probe.exists() {
        let Some(name) = probe.file_name().map(|name| name.to_os_string()) else {
            break;
        };
        suffix.push(name);
        if !probe.pop() {
            break;
        }
    }
    if probe.exists()
        && let Ok(mut canonical) = fs::canonicalize(&probe)
    {
        for component in suffix.iter().rev() {
            canonical.push(component);
        }
        return normalize_path(&canonical);
    }

    normalized
}

pub(super) fn normalize_clone_url(raw: &str) -> NormalizedCloneUrl {
    let trimmed = raw.trim();
    let without_git_prefix = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    if let Some((clone_url, secret_redacted)) = sanitize_http_clone_url(without_git_prefix) {
        return NormalizedCloneUrl {
            display: clone_url.clone(),
            clone_url: Some(clone_url),
            secret_redacted,
            local_only: false,
        };
    }
    if let Some((clone_url, secret_redacted)) = sanitize_ssh_clone_url(without_git_prefix) {
        return NormalizedCloneUrl {
            display: clone_url.clone(),
            clone_url: Some(clone_url),
            secret_redacted,
            local_only: false,
        };
    }
    if is_remote_clone_url(without_git_prefix) {
        return NormalizedCloneUrl {
            display: without_git_prefix.to_string(),
            clone_url: Some(without_git_prefix.to_string()),
            secret_redacted: false,
            local_only: false,
        };
    }
    NormalizedCloneUrl {
        display: "local-only".to_string(),
        clone_url: None,
        secret_redacted: false,
        local_only: true,
    }
}

fn sanitize_http_clone_url(raw: &str) -> Option<(String, bool)> {
    for scheme in ["https://", "http://"] {
        let Some(rest) = raw.strip_prefix(scheme) else {
            continue;
        };
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (authority, suffix) = rest.split_at(authority_end);
        let (host, had_userinfo) = authority
            .rsplit_once('@')
            .filter(|(userinfo, host)| !userinfo.is_empty() && !host.is_empty())
            .map(|(_userinfo, host)| (host, true))
            .unwrap_or((authority, false));
        let mut sanitized = format!("{scheme}{host}{suffix}");
        let mut secret_redacted = had_userinfo;
        if let Some(query_start) = sanitized.find(['?', '#']) {
            sanitized.truncate(query_start);
            secret_redacted = true;
        }
        return Some((sanitized, secret_redacted));
    }
    None
}

fn sanitize_ssh_clone_url(raw: &str) -> Option<(String, bool)> {
    let rest = raw.strip_prefix("ssh://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, suffix) = rest.split_at(authority_end);
    let (authority, mut secret_redacted) = authority
        .rsplit_once('@')
        .and_then(|(userinfo, host)| {
            let (user, password) = userinfo.split_once(':')?;
            (!user.is_empty() && !password.is_empty() && !host.is_empty())
                .then(|| (format!("{user}@{host}"), true))
        })
        .unwrap_or_else(|| (authority.to_string(), false));
    let mut sanitized = format!("ssh://{authority}{suffix}");
    if let Some(query_start) = sanitized.find(['?', '#']) {
        sanitized.truncate(query_start);
        secret_redacted = true;
    }
    Some((sanitized, secret_redacted))
}

fn is_remote_clone_url(raw: &str) -> bool {
    raw.starts_with("ssh://")
        || raw.starts_with("git://")
        || raw
            .split_once('@')
            .and_then(|(_user, host_and_path)| host_and_path.split_once(':'))
            .is_some()
}

pub(super) fn workspace_matches(kind: &str, value: &str, workspace: &Path) -> bool {
    let workspace = normalize_existing_or_raw(workspace);
    match kind {
        "path_prefix" => workspace.starts_with(normalize_existing_or_raw(Path::new(value))),
        "exact_path" => workspace == normalize_existing_or_raw(Path::new(value)),
        "name" => workspace.file_name().and_then(|name| name.to_str()) == Some(value),
        _ => false,
    }
}

pub(super) fn digest_json<T: Serialize>(value: &T) -> std::result::Result<String, CommandFailure> {
    let raw = serde_json::to_vec(value).map_err(map_io)?;
    Ok(digest_bytes(&raw))
}

pub(super) fn digest_file(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|raw| digest_bytes(&raw))
}

pub(super) fn digest_str(raw: &str) -> String {
    digest_bytes(raw.as_bytes())
}

pub(super) fn digest_bytes(raw: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(raw);
    format!("sha256:{}", to_hex(&hash.finalize()))
}

pub(super) fn shell_safe_segment(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        .collect()
}
