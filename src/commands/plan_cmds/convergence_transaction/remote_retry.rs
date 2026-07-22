use serde_json::Value;

use super::*;
use crate::core::convergence::{RemotePolicy, SkillConvergencePlan};

pub(in crate::commands::plan_cmds) fn retry_remote_transport(
    app: &App,
    stored: &Value,
    cursor: usize,
    identity: &super::super::ConvergenceApplyIdentity,
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
    let expected_binding = super::super::apply_identity::convergence_idempotency_binding_digest(
        &identity.key_digest,
        &plan.plan_id,
        &plan.plan_digest,
    )?;
    if plan.remote != RemotePolicy::Push
        || identity.plan_digest != plan.plan_digest
        || identity.binding_digest != expected_binding
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
    post_local::require_exact_transport_boundary(app, &plan, &local)?;
    let output = post_local::complete(app, &plan, &identity.key_digest, local)?;
    Ok(apply_output(&plan, cursor, identity, output))
}
