use std::fs;

use serde_json::Value;

use super::*;
use crate::core::convergence::{RemotePolicy, SkillConvergencePlan};

pub(in crate::commands::plan_cmds) fn retry_remote_transport(
    app: &App,
    stored: &Value,
    cursor: usize,
    identity: &super::super::ConvergenceApplyIdentity,
    event_local: Value,
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
        || !post_local::retry_evidence_is_valid(&plan, &event_local)
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
    let journal_path = journal_path(app, &plan.skill);
    let raw = fs::read_to_string(&journal_path).map_err(map_io)?;
    let journal: TransactionJournal = serde_json::from_str(&raw).map_err(|error| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("invalid convergence journal: {error}"),
        )
    })?;
    let mut durable_identity = identity.clone();
    recovery_identity::adopt_journal_identity(&plan, &journal, &mut durable_identity)?;
    recovery_support::validate_journal(app, &journal_path, &plan, &journal)?;
    if journal.phase != TransactionPhase::CommittedArtifactsRetained {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "pending remote retry has no retained committed transaction",
        ));
    }
    let durable_local = journal.result.clone().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "retained convergence transaction has no committed result",
        )
    })?;
    post_local::require_exact_transport_boundary(app, &plan, &durable_local)?;
    recovery_evidence::reprove_source_boundary(app, &plan, &journal)?;
    let output = post_local::complete(app, &plan, &durable_identity.key_digest, durable_local)?;
    Ok(apply_output(&plan, cursor, &durable_identity, output))
}
