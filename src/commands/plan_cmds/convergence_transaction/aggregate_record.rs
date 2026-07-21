use serde_json::json;

use crate::commands::CommandFailure;
use crate::commands::plan_cmds::ConvergenceApplyIdentity;
use crate::commands::projections::record_registry_operation;
use crate::core::convergence::SkillConvergencePlan;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

/// Intent of the single aggregate operation record written per convergence apply.
///
/// Per-projection executor records stay as-is; this record is the one row that ties a whole
/// convergence together under a single identity so `history`/audit can prove it happened once.
pub(crate) const CONVERGE_INTENT: &str = "skill.converge";

/// Write the one aggregate registry operation record for this convergence.
///
/// The raw idempotency key is never accepted here — only its digests — so it cannot reach
/// the operations log.
pub(super) fn record_convergence_operation(
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    identity: &ConvergenceApplyIdentity,
    source_commit: Option<&str>,
    registry_commit: Option<&str>,
    projection_instances: &[String],
) -> std::result::Result<String, CommandFailure> {
    record_registry_operation(
        paths,
        CONVERGE_INTENT,
        json!({
            "convergence_id": identity.convergence_id,
            "plan_id": plan.plan_id,
            "plan_digest": identity.plan_digest,
            "idempotency_key_digest": identity.key_digest,
            "idempotency_binding_digest": identity.binding_digest,
            "skill": plan.skill,
        }),
        json!({
            "source_commit": source_commit,
            "registry_commit": registry_commit,
            "projection_instances": projection_instances,
        }),
    )
    .map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to record aggregate convergence operation: {err}"),
        )
    })
}
