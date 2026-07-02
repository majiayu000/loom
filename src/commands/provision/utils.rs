use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::sha256::{Sha256, to_hex};

use super::super::helpers::{map_io, shell_arg, validate_non_empty};
use super::super::{App, CommandFailure};

pub(super) struct NormalizedCloneUrl {
    pub display: String,
    pub clone_url: String,
    pub had_userinfo: bool,
}

pub(super) fn resolve_workspace(
    app: &App,
    workspace: Option<&Path>,
) -> std::result::Result<PathBuf, CommandFailure> {
    match workspace {
        Some(path) => Ok(path.to_path_buf()),
        None => std::env::current_dir().map_err(map_io),
    }
    .map(|path| {
        if path.is_absolute() {
            path
        } else {
            app.ctx.root.join(path)
        }
    })
}

pub(super) fn validate_provision_agent(agent: &str) -> std::result::Result<(), CommandFailure> {
    validate_non_empty("agent", agent)
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

pub(super) fn target_skill_path(container_workspace: &str, agent: &str) -> String {
    format!(
        "{}/{}",
        container_workspace,
        target_skill_path_relative(agent)
    )
}

pub(super) fn target_skill_path_relative(agent: &str) -> String {
    match agent {
        "codex" => ".agents/skills".to_string(),
        "claude" => ".claude/skills".to_string(),
        other => format!(".agents/{other}/skills"),
    }
}

pub(super) fn normalize_clone_url(raw: &str) -> NormalizedCloneUrl {
    let trimmed = raw.trim();
    let without_git_prefix = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    let (clone_url, had_userinfo) = strip_url_userinfo(without_git_prefix);
    NormalizedCloneUrl {
        display: clone_url.clone(),
        clone_url,
        had_userinfo,
    }
}

fn strip_url_userinfo(raw: &str) -> (String, bool) {
    for scheme in ["https://", "http://"] {
        if let Some(rest) = raw.strip_prefix(scheme)
            && let Some((userinfo, host_and_path)) = rest.split_once('@')
            && !userinfo.is_empty()
        {
            return (format!("{scheme}{host_and_path}"), true);
        }
    }
    (raw.to_string(), false)
}

pub(super) fn workspace_matches(kind: &str, value: &str, workspace: &Path) -> bool {
    let workspace_str = workspace.display().to_string();
    match kind {
        "path_prefix" => workspace_str.starts_with(value),
        "exact_path" => workspace_str == value,
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

fn digest_bytes(raw: &[u8]) -> String {
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
