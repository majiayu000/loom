use serde_json::Value;

use crate::core::convergence::{RemotePolicy, SkillConvergencePlan};
use crate::gitops;

use super::*;

pub(in crate::commands::plan_cmds) fn retry_remote_transport(
    app: &App,
    stored: &Value,
    cursor: usize,
    idempotency_key_digest: &str,
    local: Value,
) -> std::result::Result<Value, CommandFailure> {
    let plan: SkillConvergencePlan = serde_json::from_value(stored.clone()).map_err(|err| {
        plan_failure(
            ErrorCode::StateCorrupt,
            format!("stored convergence plan is invalid: {err}"),
            "PLAN_CORRUPT",
            false,
            vec!["create and review a fresh convergence plan".to_string()],
            Some(cursor),
        )
    })?;
    let expected_binding = aggregate_audit::binding_digest(&plan, idempotency_key_digest)?;
    if plan.remote != RemotePolicy::Push
        || local["convergence_id"].as_str().is_none()
        || local["plan_digest"].as_str() != Some(plan.plan_digest.as_str())
        || local["idempotency_binding_digest"].as_str() != Some(expected_binding.as_str())
        || !post_local::retry_evidence_is_valid(&plan, &local)
    {
        return Err(plan_failure(
            ErrorCode::StateCorrupt,
            "pending remote retry is missing its exact local convergence evidence",
            "APPLY_EVENT_CORRUPT",
            false,
            vec!["inspect the retained convergence transaction".to_string()],
            Some(cursor),
        ));
    }
    let _workspace_lock = app.ctx.lock_workspace().map_err(map_lock)?;
    let _skill_lock = app.ctx.lock_skill(&plan.skill).map_err(map_lock)?;
    require_recorded_commit_ancestry(app, &local, cursor)?;
    let output = post_local::complete(app, &plan, idempotency_key_digest, local)?;
    Ok(apply_output(&plan, cursor, idempotency_key_digest, output))
}

fn require_recorded_commit_ancestry(
    app: &App,
    local: &Value,
    cursor: usize,
) -> std::result::Result<(), CommandFailure> {
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    for field in ["source_commit", "registry_commit"] {
        let value = &local[field];
        if value.is_null() {
            continue;
        }
        let commit = value.as_str().ok_or_else(|| {
            plan_failure(
                ErrorCode::StateCorrupt,
                format!("pending remote retry has invalid {field} evidence"),
                "APPLY_EVENT_CORRUPT",
                false,
                vec!["inspect the retained convergence transaction".to_string()],
                Some(cursor),
            )
        })?;
        let ancestor = gitops::run_git_allow_failure(
            &app.ctx,
            &["merge-base", "--is-ancestor", commit, &head],
        )
        .map_err(map_git)?;
        if !ancestor.status.success() {
            return Err(plan_failure(
                ErrorCode::DependencyConflict,
                format!(
                    "pending remote retry cannot prove recorded {field} {commit} is in live HEAD {head}"
                ),
                "CONVERGENCE_COMMIT_EVIDENCE_STALE",
                false,
                vec![
                    "inspect the retained convergence evidence before any remote transport"
                        .to_string(),
                ],
                Some(cursor),
            ));
        }
    }
    Ok(())
}
