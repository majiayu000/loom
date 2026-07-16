use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::commands::CommandFailure;
use crate::state::home_dir;

use super::{
    AdapterCapabilities, AdapterDiscoveryRoot, AdapterFidelity, AdapterReload, AdapterVisibility,
    ExternalAdapterV1Record, ExternalDiscoveryRootRecord, ExternalVisibilityRecord,
    adapter_failure,
};

pub(super) fn built_in_fidelity(id: &str) -> AdapterFidelity {
    if matches!(id, "claude" | "codex" | "gemini-cli") {
        AdapterFidelity::Verified
    } else {
        AdapterFidelity::Generic
    }
}

pub(super) fn built_in_default_skill_dirs(
    id: &str,
    configured: Option<&Vec<PathBuf>>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    if id != "gemini-cli" {
        return configured.cloned().unwrap_or_default();
    }
    let Some(home) = home else {
        return Vec::new();
    };
    let mut dirs = vec![home.join(".agents/skills"), home.join(".gemini/skills")];
    for path in configured.into_iter().flatten() {
        if !dirs.contains(path) {
            dirs.push(path.clone());
        }
    }
    dirs
}

pub(super) fn default_scan_eligible() -> bool {
    true
}

pub(super) fn discovery_root_json(root: &AdapterDiscoveryRoot) -> Value {
    json!({
        "scope": root.scope,
        "path_template": root.path_template,
        "role": root.role,
        "source_env_var": root.source_env_var,
        "priority": root.priority,
        "scan_eligible": root.scan_eligible,
        "available": root.available,
        "unavailable_reason": root.unavailable_reason,
    })
}

pub(super) fn visibility_json(visibility: &AdapterVisibility) -> Value {
    json!({
        "follows_symlink_dirs": visibility.follows_symlink_dirs,
        "identity_by_projection_method": visibility.identity_by_projection_method,
        "config_file": visibility.config_file,
        "disable_rules": visibility.disable_rules,
    })
}

pub(super) fn reload_json(reload: &AdapterReload) -> Value {
    json!({
        "strategy": reload.strategy,
        "hot_reload": reload.hot_reload,
        "notes": reload.notes,
    })
}

pub(super) fn role_rank(role: &str, scope: &str) -> u8 {
    match (scope, role) {
        ("user", "preferred-cross-client") => 0,
        ("project", "project-cross-client") => 0,
        (_, "env-override") => 1,
        (_, "legacy") => 2,
        (_, "legacy-default") => 3,
        (_, "manual") => 4,
        _ => 9,
    }
}

pub(super) fn resolve_root_template(
    template: &str,
    workspace: &Path,
) -> std::result::Result<PathBuf, CommandFailure> {
    let expanded = template.replace("<workspace>", &workspace.display().to_string());
    let expanded = expand_env_default(&expanded)?;
    if let Some(rest) = expanded.strip_prefix("~/") {
        return home_dir()
            .map(|home| home.join(rest))
            .ok_or_else(|| template_failure(template, "ADAPTER_HOME_UNAVAILABLE"));
    }
    if let Some(var) = expanded.strip_prefix('$') {
        let value = std::env::var(var)
            .map_err(|_| template_failure(template, "ADAPTER_ENV_UNAVAILABLE"))?;
        return Ok(PathBuf::from(value));
    }
    Ok(PathBuf::from(expanded))
}

pub(super) fn built_in_discovery_roots(
    id: &str,
    default_dirs: Option<&Vec<PathBuf>>,
    home: Option<&Path>,
) -> Vec<AdapterDiscoveryRoot> {
    if id == "codex" {
        return codex_discovery_roots(home);
    }
    if id == "claude" {
        return claude_discovery_roots(home);
    }
    if id == "gemini-cli" {
        return gemini_cli_discovery_roots(home);
    }
    default_dirs
        .into_iter()
        .flat_map(|dirs| dirs.iter())
        .map(|path| AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: path.display().to_string(),
            role: "legacy-default".to_string(),
            source_env_var: None,
            priority: None,
            scan_eligible: true,
            available: true,
            unavailable_reason: None,
        })
        .collect()
}

pub(super) fn built_in_visibility(id: &str) -> AdapterVisibility {
    if id == "codex" {
        return AdapterVisibility {
            follows_symlink_dirs: true,
            identity_by_projection_method: BTreeMap::from([
                ("symlink".to_string(), "canonical-skill-md-path".to_string()),
                ("copy".to_string(), "runtime-skill-md-path".to_string()),
                (
                    "materialize".to_string(),
                    "runtime-skill-md-path".to_string(),
                ),
            ]),
            config_file: Some("${CODEX_HOME:-~/.codex}/config.toml".to_string()),
            disable_rules: vec!["skills.config.path".to_string()],
        };
    }
    if id == "claude" {
        return AdapterVisibility {
            follows_symlink_dirs: true,
            identity_by_projection_method: BTreeMap::from([
                ("symlink".to_string(), "canonical-skill-md-path".to_string()),
                ("copy".to_string(), "runtime-skill-md-path".to_string()),
                (
                    "materialize".to_string(),
                    "runtime-skill-md-path".to_string(),
                ),
            ]),
            config_file: Some("~/.claude/settings.json".to_string()),
            disable_rules: vec!["adapter-defined".to_string()],
        };
    }
    if id == "gemini-cli" {
        return AdapterVisibility {
            follows_symlink_dirs: true,
            identity_by_projection_method: BTreeMap::from([
                ("symlink".to_string(), "canonical-skill-md-path".to_string()),
                ("copy".to_string(), "runtime-skill-md-path".to_string()),
                (
                    "materialize".to_string(),
                    "runtime-skill-md-path".to_string(),
                ),
            ]),
            config_file: Some("~/.gemini/settings.json".to_string()),
            disable_rules: vec!["adapter-defined".to_string()],
        };
    }
    default_visibility()
}

pub(super) fn built_in_reload(id: &str) -> AdapterReload {
    if id == "claude" {
        return AdapterReload {
            strategy: "new-session-recommended".to_string(),
            hot_reload: false,
            notes: Some("Claude skill visibility is session-scoped".to_string()),
        };
    }
    if id == "codex" {
        return AdapterReload {
            strategy: "new-session-recommended".to_string(),
            hot_reload: false,
            notes: Some("Codex skill visibility is session-scoped".to_string()),
        };
    }
    if id == "gemini-cli" {
        return AdapterReload {
            strategy: "in-session-command".to_string(),
            hot_reload: true,
            notes: Some(
                "Run /skills reload to refresh skills in the current Gemini CLI session"
                    .to_string(),
            ),
        };
    }
    reload_from_capability(false)
}

pub(super) fn capabilities_from_reload(reload: &AdapterReload) -> AdapterCapabilities {
    AdapterCapabilities {
        automatic_discovery: true,
        explicit_invocation: true,
        reload_required: reload.strategy == "restart-required",
    }
}

pub(super) fn default_visibility() -> AdapterVisibility {
    AdapterVisibility {
        follows_symlink_dirs: true,
        identity_by_projection_method: BTreeMap::from([(
            "default".to_string(),
            "runtime-skill-md-path".to_string(),
        )]),
        config_file: None,
        disable_rules: Vec::new(),
    }
}

pub(super) fn reload_from_capability(reload_required: bool) -> AdapterReload {
    AdapterReload {
        strategy: if reload_required {
            "restart-required"
        } else {
            "no-reload-required"
        }
        .to_string(),
        hot_reload: !reload_required,
        notes: None,
    }
}

pub(super) fn v1_discovery_roots(
    record: &ExternalAdapterV1Record,
    default_skill_dirs: &[PathBuf],
) -> Vec<AdapterDiscoveryRoot> {
    default_skill_dirs
        .iter()
        .flat_map(|path| {
            record
                .supported_scopes
                .iter()
                .map(move |scope| AdapterDiscoveryRoot {
                    scope: scope.clone(),
                    path_template: path.display().to_string(),
                    role: "legacy-default".to_string(),
                    source_env_var: None,
                    priority: None,
                    scan_eligible: scope == "user" || record.supported_scopes.len() == 1,
                    available: true,
                    unavailable_reason: None,
                })
        })
        .collect()
}

pub(super) fn external_discovery_root(
    root: &ExternalDiscoveryRootRecord,
    home: Option<&Path>,
    config_path: &Path,
) -> std::result::Result<AdapterDiscoveryRoot, CommandFailure> {
    if root.path_template.trim().is_empty() {
        return Err(adapter_failure(
            "adapter discovery root path must not be empty",
            "ADAPTER_DISCOVERY_PATH_INVALID",
            Some(config_path),
        ));
    }
    let env_available = root
        .source_env_var
        .as_ref()
        .is_none_or(|var| std::env::var_os(var).is_some());
    let home_available = home.is_some() || !root.path_template.starts_with("~/");
    let available = env_available && home_available;
    let unavailable_reason = if !env_available {
        root.source_env_var
            .as_ref()
            .map(|var| format!("{var} is not set"))
    } else if !home_available {
        Some("HOME or USERPROFILE is not set".to_string())
    } else {
        None
    };
    Ok(AdapterDiscoveryRoot {
        scope: root.scope.clone(),
        path_template: root.path_template.clone(),
        role: root.role.clone(),
        source_env_var: root.source_env_var.clone(),
        priority: root.priority,
        scan_eligible: root.scan_eligible,
        available,
        unavailable_reason,
    })
}

pub(super) fn external_visibility(record: ExternalVisibilityRecord) -> AdapterVisibility {
    let mut identity_by_projection_method = record.identity_by_projection_method;
    if identity_by_projection_method.is_empty() {
        if let Some(identity) = record.identity {
            identity_by_projection_method.insert("default".to_string(), identity);
        } else {
            identity_by_projection_method = default_visibility().identity_by_projection_method;
        }
    }
    AdapterVisibility {
        follows_symlink_dirs: record.follows_symlink_dirs,
        identity_by_projection_method,
        config_file: record.config_file,
        disable_rules: record.disable_rules,
    }
}

pub(super) fn validate_discovery_root(
    root: &ExternalDiscoveryRootRecord,
    supported_scopes: &[String],
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    validate_allowed(
        "discovery_roots.scope",
        &root.scope,
        &["user", "project"],
        path,
    )?;
    if !supported_scopes.iter().any(|scope| scope == &root.scope) {
        return Err(adapter_failure(
            format!(
                "adapter discovery root scope '{}' is not in supported_scopes",
                root.scope
            ),
            "ADAPTER_FIELD_UNSUPPORTED",
            Some(path),
        ));
    }
    validate_allowed(
        "discovery_roots.role",
        &root.role,
        &[
            "preferred-cross-client",
            "project-cross-client",
            "legacy",
            "legacy-default",
            "env-override",
            "manual",
        ],
        path,
    )?;
    if root.path_template.trim().is_empty() {
        return Err(adapter_failure(
            "adapter discovery root path must not be empty",
            "ADAPTER_DISCOVERY_PATH_INVALID",
            Some(path),
        ));
    }
    Ok(())
}

pub(super) fn validate_visibility(
    visibility: &ExternalVisibilityRecord,
    projection_methods: &[String],
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    let allowed = [
        "canonical-skill-md-path",
        "runtime-skill-md-path",
        "directory-path",
        "adapter-defined",
    ];
    if let Some(identity) = &visibility.identity {
        validate_allowed("visibility.identity", identity, &allowed, path)?;
    }
    for (method, identity) in &visibility.identity_by_projection_method {
        if method != "default" && !projection_methods.iter().any(|known| known == method) {
            return Err(adapter_failure(
                format!("visibility identity references unsupported projection method '{method}'"),
                "ADAPTER_FIELD_UNSUPPORTED",
                Some(path),
            ));
        }
        validate_allowed(
            "visibility.identity_by_projection_method",
            identity,
            &allowed,
            path,
        )?;
    }
    for rule in &visibility.disable_rules {
        validate_allowed(
            "visibility.disable_rules",
            rule,
            &["skills.config.path", "adapter-defined"],
            path,
        )?;
    }
    Ok(())
}

pub(super) fn adapter_json_invalid(path: &Path, err: serde_json::Error) -> CommandFailure {
    adapter_failure(
        format!("adapter config '{}' is invalid JSON: {err}", path.display()),
        "ADAPTER_JSON_INVALID",
        Some(path),
    )
}

fn codex_discovery_roots(home: Option<&Path>) -> Vec<AdapterDiscoveryRoot> {
    let code_home_available = std::env::var_os("CODEX_HOME").is_some() || home.is_some();
    vec![
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "~/.agents/skills".to_string(),
            role: "preferred-cross-client".to_string(),
            source_env_var: None,
            priority: Some(0),
            scan_eligible: true,
            available: home.is_some(),
            unavailable_reason: home
                .is_none()
                .then(|| "HOME or USERPROFILE is not set".to_string()),
        },
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "$CODEX_SKILLS_DIR".to_string(),
            role: "env-override".to_string(),
            source_env_var: Some("CODEX_SKILLS_DIR".to_string()),
            priority: Some(1),
            scan_eligible: true,
            available: std::env::var_os("CODEX_SKILLS_DIR").is_some(),
            unavailable_reason: std::env::var_os("CODEX_SKILLS_DIR")
                .is_none()
                .then(|| "CODEX_SKILLS_DIR is not set".to_string()),
        },
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "${CODEX_HOME:-~/.codex}/skills".to_string(),
            role: "legacy".to_string(),
            source_env_var: Some("CODEX_HOME".to_string()),
            priority: Some(2),
            scan_eligible: true,
            available: code_home_available,
            unavailable_reason: (!code_home_available)
                .then(|| "CODEX_HOME and HOME/USERPROFILE are not set".to_string()),
        },
        AdapterDiscoveryRoot {
            scope: "project".to_string(),
            path_template: "<workspace>/.agents/skills".to_string(),
            role: "project-cross-client".to_string(),
            source_env_var: None,
            priority: Some(0),
            scan_eligible: false,
            available: true,
            unavailable_reason: None,
        },
    ]
}

fn claude_discovery_roots(home: Option<&Path>) -> Vec<AdapterDiscoveryRoot> {
    let claude_home_available = std::env::var_os("CLAUDE_HOME").is_some() || home.is_some();
    vec![
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "~/.claude/skills".to_string(),
            role: "preferred-cross-client".to_string(),
            source_env_var: None,
            priority: Some(0),
            scan_eligible: true,
            available: home.is_some(),
            unavailable_reason: home
                .is_none()
                .then(|| "HOME or USERPROFILE is not set".to_string()),
        },
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "$CLAUDE_SKILLS_DIR".to_string(),
            role: "env-override".to_string(),
            source_env_var: Some("CLAUDE_SKILLS_DIR".to_string()),
            priority: Some(1),
            scan_eligible: true,
            available: std::env::var_os("CLAUDE_SKILLS_DIR").is_some(),
            unavailable_reason: std::env::var_os("CLAUDE_SKILLS_DIR")
                .is_none()
                .then(|| "CLAUDE_SKILLS_DIR is not set".to_string()),
        },
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "${CLAUDE_HOME:-~/.claude}/skills".to_string(),
            role: "legacy".to_string(),
            source_env_var: Some("CLAUDE_HOME".to_string()),
            priority: Some(2),
            scan_eligible: true,
            available: claude_home_available,
            unavailable_reason: (!claude_home_available)
                .then(|| "CLAUDE_HOME and HOME/USERPROFILE are not set".to_string()),
        },
        AdapterDiscoveryRoot {
            scope: "project".to_string(),
            path_template: "<workspace>/.claude/skills".to_string(),
            role: "project-cross-client".to_string(),
            source_env_var: None,
            priority: Some(0),
            scan_eligible: false,
            available: true,
            unavailable_reason: None,
        },
    ]
}

fn gemini_cli_discovery_roots(home: Option<&Path>) -> Vec<AdapterDiscoveryRoot> {
    let unavailable_reason = || {
        home.is_none()
            .then(|| "HOME or USERPROFILE is not set".to_string())
    };
    vec![
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "~/.agents/skills".to_string(),
            role: "preferred-cross-client".to_string(),
            source_env_var: None,
            priority: Some(0),
            scan_eligible: true,
            available: home.is_some(),
            unavailable_reason: unavailable_reason(),
        },
        AdapterDiscoveryRoot {
            scope: "user".to_string(),
            path_template: "~/.gemini/skills".to_string(),
            role: "preferred-cross-client".to_string(),
            source_env_var: None,
            priority: Some(1),
            scan_eligible: true,
            available: home.is_some(),
            unavailable_reason: unavailable_reason(),
        },
        AdapterDiscoveryRoot {
            scope: "project".to_string(),
            path_template: "<workspace>/.agents/skills".to_string(),
            role: "project-cross-client".to_string(),
            source_env_var: None,
            priority: Some(0),
            scan_eligible: false,
            available: true,
            unavailable_reason: None,
        },
        AdapterDiscoveryRoot {
            scope: "project".to_string(),
            path_template: "<workspace>/.gemini/skills".to_string(),
            role: "project-cross-client".to_string(),
            source_env_var: None,
            priority: Some(1),
            scan_eligible: false,
            available: true,
            unavailable_reason: None,
        },
    ]
}

fn validate_allowed(
    field: &str,
    value: &str,
    allowed: &[&str],
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(adapter_failure(
        format!("adapter field '{field}' contains unsupported value '{value}'"),
        "ADAPTER_FIELD_UNSUPPORTED",
        Some(path),
    ))
}

fn expand_env_default(raw: &str) -> std::result::Result<String, CommandFailure> {
    let Some(start) = raw.find("${") else {
        return Ok(raw.to_string());
    };
    let Some(end_offset) = raw[start + 2..].find('}') else {
        return Ok(raw.to_string());
    };
    let end = start + 2 + end_offset;
    let expr = &raw[start + 2..end];
    let replacement = if let Some((var, fallback)) = expr.split_once(":-") {
        std::env::var(var).unwrap_or_else(|_| fallback.to_string())
    } else {
        std::env::var(expr).map_err(|_| template_failure(raw, "ADAPTER_ENV_UNAVAILABLE"))?
    };
    Ok(format!(
        "{}{}{}",
        &raw[..start],
        replacement,
        &raw[end + 1..]
    ))
}

fn template_failure(template: &str, reason: &str) -> CommandFailure {
    adapter_failure(
        format!("adapter discovery root '{template}' could not be resolved"),
        reason,
        None,
    )
}
