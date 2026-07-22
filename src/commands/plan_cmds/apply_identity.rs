use serde_json::{Value, json};

use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

use super::{CommandFailure, converge, plan_failure};

/// Identity carried through a convergence apply so every persisted surface agrees.
#[derive(Clone, Debug)]
pub(crate) struct ConvergenceApplyIdentity {
    pub key_digest: String,
    pub binding_digest: String,
    pub plan_digest: String,
    pub convergence_id: String,
}

pub(super) fn idempotency_key_digest(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

/// Bind a non-convergence idempotency key to the durable plan it was confirmed against.
pub(super) fn idempotency_binding_digest(
    key_digest: &str,
    plan_id: &str,
    plan_digest: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key_digest.as_bytes());
    hasher.update(b"\n");
    hasher.update(plan_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(plan_digest.as_bytes());
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

pub(super) fn convergence_idempotency_binding_digest(
    key_digest: &str,
    plan_id: &str,
    plan_digest: &str,
) -> std::result::Result<String, CommandFailure> {
    converge::digest_value(&json!({
        "kind": "loom.convergence.apply.v1",
        "plan_id": plan_id,
        "plan_digest": plan_digest,
        "idempotency_key_digest": key_digest,
    }))
}

/// Deterministic identity for one convergence plan and idempotency binding.
pub(crate) fn convergence_id(binding_digest: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"loom.convergence.v1\n");
    hasher.update(binding_digest.as_bytes());
    format!("conv_{}", &to_hex(&hasher.finalize())[..32])
}

pub(super) fn replay_convergence_identity(
    replay: &Value,
    key_digest: &str,
    binding_digest: &str,
    plan_digest: &str,
) -> std::result::Result<ConvergenceApplyIdentity, CommandFailure> {
    let convergence_id = replay["convergence_id"].as_str().filter(|value| {
        value.strip_prefix("conv_").is_some_and(|suffix| {
            suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    });
    let Some(convergence_id) = convergence_id else {
        return Err(plan_failure(
            ErrorCode::StateCorrupt,
            "prior convergence apply is missing its recorded convergence identity",
            "APPLY_EVENT_CORRUPT",
            false,
            vec!["inspect the retained convergence evidence".to_string()],
            None,
        ));
    };
    Ok(ConvergenceApplyIdentity {
        key_digest: key_digest.to_string(),
        binding_digest: binding_digest.to_string(),
        plan_digest: plan_digest.to_string(),
        convergence_id: convergence_id.to_string(),
    })
}
