use crate::commands::skill_policy::SkillPolicyReport;
use crate::core::convergence::ConvergencePreflightEvidence;

use super::{CommandFailure, digest_value, json, map_io};

pub(super) fn seal_policy_gate(
    preflight: &mut ConvergencePreflightEvidence,
    policy: &SkillPolicyReport,
    required_approvals: &[String],
) -> std::result::Result<(), CommandFailure> {
    preflight.checks.extend([
        (
            "policy_safe_capture_digest".to_string(),
            digest_value(&serde_json::to_value(policy).map_err(map_io)?)?,
        ),
        (
            "policy_decision".to_string(),
            if policy.allowed { "allowed" } else { "blocked" }.to_string(),
        ),
        (
            "policy_required_approvals_digest".to_string(),
            digest_value(&json!(required_approvals))?,
        ),
    ]);
    Ok(())
}
