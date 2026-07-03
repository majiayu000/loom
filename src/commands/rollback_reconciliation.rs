use std::fs;

use serde_json::{Value, json};

use crate::state_model::{RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths};

use super::helpers::shell_arg;
use super::projections::record_registry_observation;

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
        let item = rollback_projection_item(ctx, projection);
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
    projection: &RegistryProjectionInstance,
) -> Value {
    let method = projection.method.as_str();
    let content_projection = method == "copy" || method == "materialize";
    let live_path_exists = fs::metadata(&projection.materialized_path).is_ok();
    let requires_projection_reapply = content_projection || !live_path_exists;
    let status = if !live_path_exists {
        "missing_projection_path"
    } else if content_projection {
        "requires_reapply"
    } else {
        "symlink_noop"
    };
    let next_action = if requires_projection_reapply {
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
        "next_action": next_action
    })
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
