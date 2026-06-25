use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::super::CommandFailure;
use super::super::helpers::{map_registry_state, projection_method_as_str, shell_arg};
use crate::cli::AgentPreflightArgs;
use crate::state_model::{RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths};

pub(super) fn push_target_risks(
    risks: &mut Vec<Value>,
    snapshot: &RegistrySnapshot,
    binding_id: &str,
    target_id: &str,
    method: &str,
) {
    let Some(target) = snapshot.target(target_id) else {
        risks.push(risk(
            "error",
            "TARGET_NOT_FOUND",
            format!("target '{}' not found", target_id),
        ));
        return;
    };
    if target.ownership != "managed" {
        risks.push(risk(
            "error",
            "TARGET_NOT_MANAGED",
            format!(
                "target '{}' has ownership '{}' and cannot be written",
                target.target_id, target.ownership
            ),
        ));
    }
    if let Some(binding) = snapshot.binding(binding_id)
        && target.agent != binding.agent
    {
        risks.push(risk(
            "error",
            "TARGET_AGENT_MISMATCH",
            format!(
                "binding '{}' is for agent '{}' but target '{}' is for agent '{}'",
                binding.binding_id, binding.agent, target.target_id, target.agent
            ),
        ));
    }
    match method {
        "symlink" if !target.capabilities.symlink => risks.push(risk(
            "error",
            "PROJECTION_METHOD_UNSUPPORTED",
            format!(
                "target '{}' does not support symlink projections",
                target.target_id
            ),
        )),
        "copy" | "materialize" if !target.capabilities.copy => risks.push(risk(
            "error",
            "PROJECTION_METHOD_UNSUPPORTED",
            format!(
                "target '{}' does not support copy/materialize projections",
                target.target_id
            ),
        )),
        _ => {}
    }
}

pub(super) fn rollback_impacted_projections(
    ctx: &crate::state::AppContext,
    skill: &str,
) -> std::result::Result<(Vec<Value>, Vec<String>), CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let Some(snapshot) = paths.maybe_load_snapshot().map_err(map_registry_state)? else {
        return Ok((
            Vec::new(),
            vec![format!(
                "registry state not initialized under {}; impacted projections could not be determined",
                paths.registry_dir.display()
            )],
        ));
    };
    let projections = snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill)
        .map(|projection| {
            let requires_reproject = matches!(projection.method.as_str(), "copy" | "materialize");
            json!({
                "instance_id": projection.instance_id,
                "binding_id": projection.binding_id.as_deref(),
                "target_id": projection.target_id,
                "method": projection.method,
                "live_path": projection.materialized_path,
                "requires_reproject": requires_reproject,
            })
        })
        .collect();
    Ok((projections, Vec::new()))
}

pub(super) fn build_preflight_next_commands(
    root: &Path,
    args: &AgentPreflightArgs,
    selectors: &Value,
) -> Vec<String> {
    let Some(skill) = args.skill.as_deref() else {
        return Vec::new();
    };
    let Some(binding_id) = selectors["binding_id"].as_str() else {
        return Vec::new();
    };
    if selectors["target_id"].is_null() {
        return Vec::new();
    }
    let method = selectors["method"]
        .as_str()
        .unwrap_or_else(|| projection_method_as_str(args.method));
    let mut command = format!(
        "loom --json --root {} skill project {} --binding {} --method {}",
        shell_arg(root),
        shell_arg(skill),
        shell_arg(binding_id),
        method
    );
    if let Some(target_id) = selectors["target_id"].as_str() {
        command.push_str(&format!(" --target {}", shell_arg(target_id)));
    }
    vec![command]
}

pub(super) fn target_paths(matches: &[Value]) -> Vec<String> {
    matches
        .iter()
        .filter_map(|entry| entry["target"]["path"].as_str().map(ToString::to_string))
        .collect()
}

pub(super) fn normalize_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    absolute.canonicalize().unwrap_or(absolute)
}

pub(super) fn workspace_matches(kind: &str, value: &str, workspace: &Path) -> bool {
    match kind {
        "path_prefix" => workspace.starts_with(normalize_path(Path::new(value))),
        "exact_path" => workspace == normalize_path(Path::new(value)),
        "name" => workspace.file_name().and_then(|name| name.to_str()) == Some(value),
        _ => false,
    }
}

pub(super) fn is_orphan_projection(projection: &RegistryProjectionInstance) -> bool {
    projection.binding_id.is_none() && projection.health == "orphaned"
}

pub(super) fn is_safe(risks: &[Value]) -> bool {
    !risks
        .iter()
        .any(|risk| risk["severity"].as_str() == Some("error"))
}

pub(super) fn status_for(risks: &[Value], match_count: usize) -> &'static str {
    if !is_safe(risks) {
        return "blocked";
    }
    if match_count == 0 {
        return "no-op";
    }
    if risks
        .iter()
        .any(|risk| risk["severity"].as_str() == Some("warning"))
    {
        return "ready_with_warnings";
    }
    "ready"
}

pub(super) fn risk(
    severity: &'static str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Value {
    json!({
        "severity": severity,
        "code": code.into(),
        "message": message.into(),
    })
}
