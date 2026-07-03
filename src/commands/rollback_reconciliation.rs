use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::state_model::{
    RegistryOperationRecord, RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths,
};

use super::helpers::shell_arg;
use super::projections::record_registry_observation;

const COMPILED_METADATA_DIR: &str = ".loom/compiled";
const COMPILED_PROJECTION_KIND: &str = "compiled_activation";
const COMPILED_PROJECTION_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone)]
struct CompiledProjectionRecovery {
    metadata_source: &'static str,
    artifact_id: Option<String>,
    artifact_path: Option<String>,
    agent: Option<String>,
    scope: Option<String>,
    profile: Option<String>,
    target_id: Option<String>,
    workspace: Option<String>,
    reason: Option<String>,
}

pub(crate) fn rollback_noop_projection_reconciliation() -> Value {
    json!({
        "status": "noop",
        "mode": "recovery_plan_only",
        "items": [],
        "next_actions": [],
        "requires_projection_reapply": false,
        "live_projection_reconciled": true,
        "error": Value::Null
    })
}

pub(crate) fn rollback_projection_reconciliation(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    skill_id: &str,
    registry_existed_before: bool,
) -> (Value, Vec<String>) {
    if !registry_existed_before {
        return rollback_projection_registry_missing(
            paths,
            "registry state was not initialized before rollback",
        );
    }
    if rollback_fault_active("projection_reconciliation_snapshot_load") {
        return rollback_projection_registry_unavailable(
            "fault injected at projection_reconciliation_snapshot_load".to_string(),
        );
    }
    match paths.maybe_load_snapshot() {
        Ok(Some(snapshot)) => {
            rollback_projection_reconciliation_from_snapshot(ctx, skill_id, &snapshot)
        }
        Ok(None) => rollback_projection_registry_missing(paths, "registry state is missing"),
        Err(err) => rollback_projection_registry_unavailable(err.to_string()),
    }
}

fn rollback_projection_registry_missing(
    paths: &RegistryStatePaths,
    reason: &str,
) -> (Value, Vec<String>) {
    let warning = format!(
        "rollback could not determine live projection reconciliation because {reason} under {}",
        paths.registry_dir.display()
    );
    (
        json!({
            "status": "registry_missing",
            "mode": "recovery_plan_only",
            "items": [],
            "next_actions": [{
                "type": "manual_review_required",
                "reason": "registry state is missing; inspect live agent skill directories before assuming rollback updated projected content"
            }],
            "requires_projection_reapply": false,
            "live_projection_reconciled": false,
            "error": Value::Null
        }),
        vec![warning],
    )
}

fn rollback_projection_registry_unavailable(message: String) -> (Value, Vec<String>) {
    let warning = format!(
        "rollback could not determine live projection reconciliation because registry snapshot loading failed: {message}"
    );
    (
        json!({
            "status": "registry_unavailable",
            "mode": "recovery_plan_only",
            "items": [],
            "next_actions": [{
                "type": "manual_review_required",
                "reason": "registry snapshot could not be loaded; inspect live agent skill directories before assuming rollback updated projected content"
            }],
            "requires_projection_reapply": false,
            "live_projection_reconciled": false,
            "error": {
                "code": "REGISTRY_STATE_UNAVAILABLE",
                "message": message
            }
        }),
        vec![warning],
    )
}

fn rollback_projection_reconciliation_from_snapshot(
    ctx: &crate::state::AppContext,
    skill_id: &str,
    snapshot: &RegistrySnapshot,
) -> (Value, Vec<String>) {
    let mut items = Vec::new();
    let mut next_actions = Vec::new();
    for projection in snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill_id)
    {
        let item = rollback_projection_item(ctx, snapshot, projection);
        if let Some(action) = item["next_action"].as_object() {
            next_actions.push(Value::Object(action.clone()));
        }
        items.push(item);
    }

    let reapply_ids = items
        .iter()
        .filter(|item| item["requires_projection_reapply"].as_bool() == Some(true))
        .filter_map(|item| item["instance_id"].as_str().map(ToString::to_string))
        .collect::<Vec<_>>();
    let requires_projection_reapply = !reapply_ids.is_empty();
    let status = if requires_projection_reapply {
        "requires_reapply"
    } else {
        "verified_no_reapply_required"
    };
    let warnings = if requires_projection_reapply {
        vec![format!(
            "rollback restored source but live projections require reapply or manual review; run projection_reconciliation.next_actions for: {}",
            reapply_ids.join(", ")
        )]
    } else {
        Vec::new()
    };

    (
        json!({
            "status": status,
            "mode": "recovery_plan_only",
            "items": items,
            "next_actions": next_actions,
            "requires_projection_reapply": requires_projection_reapply,
            "live_projection_reconciled": !requires_projection_reapply,
            "error": Value::Null
        }),
        warnings,
    )
}

fn rollback_projection_item(
    ctx: &crate::state::AppContext,
    snapshot: &RegistrySnapshot,
    projection: &RegistryProjectionInstance,
) -> Value {
    let method = projection.method.as_str();
    let content_projection = method == "copy" || method == "materialize";
    let compiled_recovery = if content_projection {
        compiled_projection_recovery(snapshot, projection)
    } else {
        None
    };
    let status = if content_projection {
        if fs::metadata(&projection.materialized_path).is_ok() {
            "requires_reapply"
        } else {
            "missing_projection_path"
        }
    } else {
        symlink_projection_status(ctx, projection)
    };
    let live_path_exists = status != "missing_projection_path";
    let requires_projection_reapply = content_projection || status != "symlink_noop";
    let status = if content_projection && live_path_exists {
        "requires_reapply"
    } else {
        status
    };
    let next_action = if requires_projection_reapply {
        rollback_projection_next_action(ctx, projection, compiled_recovery.as_ref())
    } else {
        Value::Null
    };

    json!({
        "instance_id": projection.instance_id,
        "skill_id": projection.skill_id,
        "binding_id": projection.binding_id,
        "target_id": projection.target_id,
        "materialized_path": projection.materialized_path,
        "method": projection.method,
        "status": status,
        "live_path_exists": live_path_exists,
        "requires_projection_reapply": requires_projection_reapply,
        "compiled_activation": compiled_recovery.as_ref().map(compiled_projection_recovery_value),
        "next_action": next_action
    })
}

fn symlink_projection_status(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
) -> &'static str {
    let Ok(metadata) = fs::symlink_metadata(&projection.materialized_path) else {
        return "missing_projection_path";
    };
    if !metadata.file_type().is_symlink() {
        return "not_symlink";
    }
    let Ok(actual) = fs::canonicalize(&projection.materialized_path) else {
        return "missing_projection_path";
    };
    let Ok(expected) = fs::canonicalize(ctx.root.join("skills").join(&projection.skill_id)) else {
        return "missing_projection_path";
    };
    if actual == expected {
        "symlink_noop"
    } else {
        "symlink_target_mismatch"
    }
}

fn rollback_projection_command(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
    binding_id: &str,
) -> String {
    format!(
        "loom --json --root {} skill project {} --binding {} --target {} --method {}",
        shell_arg(&ctx.root),
        shell_arg(&projection.skill_id),
        shell_arg(binding_id),
        shell_arg(&projection.target_id),
        projection.method.as_str()
    )
}

fn rollback_projection_next_action(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
    compiled_recovery: Option<&CompiledProjectionRecovery>,
) -> Value {
    if let Some(compiled_recovery) = compiled_recovery {
        return compiled_projection_next_action(ctx, projection, compiled_recovery);
    }
    projection
        .binding_id
        .as_deref()
        .map(|binding_id| {
            json!({
                "type": "command",
                "instance_id": projection.instance_id,
                "command": rollback_projection_command(ctx, projection, binding_id)
            })
        })
        .unwrap_or_else(|| {
            json!({
                "type": "manual_review_required",
                "instance_id": projection.instance_id,
                "reason": "projection is orphaned and has no binding_id; inspect or clean the projection manually"
            })
        })
}

fn compiled_projection_next_action(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
    recovery: &CompiledProjectionRecovery,
) -> Value {
    let mut action = json!({
        "type": "manual_review_required",
        "instance_id": projection.instance_id,
        "reason": "projection was produced by compiled activation; do not recover it with raw skill project materialize because that would replace the compiled artifact view with source materialization",
        "compiled": compiled_projection_recovery_value(recovery)
    });
    if recovery.reason.is_none() {
        let commands = compiled_projection_recovery_commands(ctx, projection, recovery);
        if !commands.is_empty() {
            action["commands"] = json!(commands);
        }
    }
    action
}

fn compiled_projection_recovery(
    snapshot: &RegistrySnapshot,
    projection: &RegistryProjectionInstance,
) -> Option<CompiledProjectionRecovery> {
    compiled_projection_recovery_from_operation(snapshot, projection)
        .or_else(|| compiled_projection_recovery_from_live_path(projection))
}

fn compiled_projection_recovery_from_operation(
    snapshot: &RegistrySnapshot,
    projection: &RegistryProjectionInstance,
) -> Option<CompiledProjectionRecovery> {
    let op = snapshot
        .operations
        .iter()
        .rev()
        .find(|op| compiled_activation_op_matches_projection(op, projection))?;
    let compiled = &op.payload["compiled"];
    Some(CompiledProjectionRecovery {
        metadata_source: "registry_operation",
        artifact_id: compiled["artifact_id"].as_str().map(ToString::to_string),
        artifact_path: compiled["artifact_path"].as_str().map(ToString::to_string),
        agent: op.payload["agent"].as_str().map(ToString::to_string),
        scope: op.payload["scope"].as_str().map(ToString::to_string),
        profile: op.payload["profile"].as_str().map(ToString::to_string),
        target_id: op.payload["target_id"].as_str().map(ToString::to_string),
        workspace: compiled_projection_workspace(snapshot, op.payload["binding_id"].as_str()),
        reason: None,
    })
}

fn compiled_activation_op_matches_projection(
    op: &RegistryOperationRecord,
    projection: &RegistryProjectionInstance,
) -> bool {
    op.intent == "skill.activate"
        && op.status == "succeeded"
        && op.effects["instance_id"].as_str() == Some(projection.instance_id.as_str())
        && op.payload["compiled"]["projection_kind"].as_str() == Some(COMPILED_PROJECTION_KIND)
}

fn compiled_projection_recovery_from_live_path(
    projection: &RegistryProjectionInstance,
) -> Option<CompiledProjectionRecovery> {
    let metadata_path = Path::new(&projection.materialized_path)
        .join(COMPILED_METADATA_DIR)
        .join("projection.json");
    if !metadata_path.exists() {
        return None;
    }
    let raw = match fs::read_to_string(&metadata_path) {
        Ok(raw) => raw,
        Err(err) => {
            return Some(compiled_projection_manual_recovery(format!(
                "compiled projection metadata exists but could not be read at {}: {err}",
                metadata_path.display()
            )));
        }
    };
    let value = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => value,
        Err(err) => {
            return Some(compiled_projection_manual_recovery(format!(
                "compiled projection metadata exists but could not be parsed at {}: {err}",
                metadata_path.display()
            )));
        }
    };
    if value["schema_version"].as_u64() != Some(COMPILED_PROJECTION_SCHEMA_VERSION)
        || value["kind"].as_str() != Some(COMPILED_PROJECTION_KIND)
    {
        return Some(compiled_projection_manual_recovery(format!(
            "compiled projection metadata at {} is not a valid compiled activation marker",
            metadata_path.display()
        )));
    }
    Some(CompiledProjectionRecovery {
        metadata_source: "live_projection_metadata",
        artifact_id: value["artifact_id"].as_str().map(ToString::to_string),
        artifact_path: value["artifact_path"].as_str().map(ToString::to_string),
        agent: value["agent"].as_str().map(ToString::to_string),
        scope: None,
        profile: value["profile"].as_str().map(ToString::to_string),
        target_id: None,
        workspace: None,
        reason: None,
    })
}

fn compiled_projection_manual_recovery(reason: String) -> CompiledProjectionRecovery {
    CompiledProjectionRecovery {
        metadata_source: "live_projection_metadata",
        artifact_id: None,
        artifact_path: None,
        agent: None,
        scope: None,
        profile: None,
        target_id: None,
        workspace: None,
        reason: Some(reason),
    }
}

fn compiled_projection_workspace(
    snapshot: &RegistrySnapshot,
    binding_id: Option<&str>,
) -> Option<String> {
    let binding_id = binding_id?;
    let binding = snapshot
        .bindings
        .bindings
        .iter()
        .find(|binding| binding.binding_id == binding_id)?;
    match binding.workspace_matcher.kind.as_str() {
        "path_prefix" | "exact_path" => Some(binding.workspace_matcher.value.clone()),
        _ => None,
    }
}

fn compiled_projection_recovery_commands(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
    recovery: &CompiledProjectionRecovery,
) -> Vec<String> {
    let Some(artifact_id) = recovery.artifact_id.as_deref() else {
        return Vec::new();
    };
    let verify = format!(
        "loom --json --root {} skill compile verify {} --artifact {}",
        shell_arg(&ctx.root),
        shell_arg(&projection.skill_id),
        shell_arg(artifact_id)
    );
    let Some(agent) = recovery.agent.as_deref() else {
        return vec![verify];
    };
    let Some(scope) = recovery.scope.as_deref() else {
        return vec![verify];
    };
    let Some(target_id) = recovery.target_id.as_deref() else {
        return vec![verify];
    };
    if scope == "project" && recovery.workspace.is_none() {
        return vec![verify];
    }
    let mut activate = format!(
        "loom --json --root {} skill activate {} --agent {} --compiled --artifact {}",
        shell_arg(&ctx.root),
        shell_arg(&projection.skill_id),
        shell_arg(agent),
        shell_arg(artifact_id)
    );
    activate.push_str(&format!(" --scope {}", shell_arg(scope)));
    if let Some(workspace) = recovery.workspace.as_deref() {
        activate.push_str(&format!(" --workspace {}", shell_arg(workspace)));
    }
    if let Some(profile) = recovery.profile.as_deref() {
        activate.push_str(&format!(" --profile {}", shell_arg(profile)));
    }
    activate.push_str(&format!(" --target {}", shell_arg(target_id)));
    vec![verify, activate]
}

fn compiled_projection_recovery_value(recovery: &CompiledProjectionRecovery) -> Value {
    json!({
        "projection_kind": COMPILED_PROJECTION_KIND,
        "metadata_source": recovery.metadata_source,
        "artifact_id": recovery.artifact_id,
        "artifact_path": recovery.artifact_path,
        "agent": recovery.agent,
        "scope": recovery.scope,
        "profile": recovery.profile,
        "target_id": recovery.target_id,
        "workspace": recovery.workspace,
        "reason": recovery.reason
    })
}

pub(crate) fn record_skill_projection_observations(
    paths: &RegistryStatePaths,
    skill_id: &str,
    kind: &str,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> anyhow::Result<()> {
    if let Some(snapshot) = paths.maybe_load_snapshot()? {
        record_skill_projection_observations_from_snapshot(
            paths, skill_id, kind, path, from, to, &snapshot,
        )?;
    }
    Ok(())
}

pub(crate) fn record_skill_projection_observations_for_rollback(
    paths: &RegistryStatePaths,
    skill_id: &str,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> anyhow::Result<Vec<String>> {
    let snapshot = match paths.maybe_load_snapshot() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return Ok(Vec::new()),
        Err(err) => {
            return Ok(vec![format!(
                "rollback could not record projection observations because registry snapshot loading failed: {err}"
            )]);
        }
    };
    record_skill_projection_observations_from_snapshot(
        paths, skill_id, "rollback", path, from, to, &snapshot,
    )?;
    Ok(Vec::new())
}

fn record_skill_projection_observations_from_snapshot(
    paths: &RegistryStatePaths,
    skill_id: &str,
    kind: &str,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
    snapshot: &RegistrySnapshot,
) -> anyhow::Result<()> {
    for projection in snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill_id)
    {
        record_registry_observation(
            paths,
            &projection.instance_id,
            kind,
            path.clone(),
            from.clone(),
            to.clone(),
        )?;
    }
    Ok(())
}

fn rollback_fault_active(tag: &str) -> bool {
    std::env::var("LOOM_ROLLBACK_FAULT_INJECT")
        .ok()
        .map(|value| value.split(',').any(|item| item.trim() == tag))
        .unwrap_or(false)
}
