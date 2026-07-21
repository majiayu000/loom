use super::*;
use crate::commands::projections::record_registry_operation_with_id;

pub(super) const OPERATIONS_PATH: &str = "state/registry/ops/operations.jsonl";
pub(super) const CHECKPOINT_PATH: &str = "state/registry/ops/checkpoint.json";

pub(super) fn is_registry_commit_path(path: &str) -> bool {
    matches!(
        path,
        "state/registry/projections.json" | OPERATIONS_PATH | CHECKPOINT_PATH
    )
}

pub(super) fn binding_digest(
    plan: &SkillConvergencePlan,
    key_digest: &str,
) -> std::result::Result<String, CommandFailure> {
    digest_value(&json!({
        "kind": "loom.convergence.apply.v1",
        "plan_id": plan.plan_id,
        "plan_digest": plan.plan_digest,
        "idempotency_key_digest": key_digest,
    }))
}

pub(super) fn new_convergence_id() -> String {
    format!("conv_{}", uuid::Uuid::new_v4().simple())
}

fn operation_id(convergence_id: &str) -> String {
    format!(
        "op_{}",
        convergence_id
            .strip_prefix("conv_")
            .unwrap_or(convergence_id)
    )
}

pub(super) fn record(
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    local_axes: &Value,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let operation_id = operation_id(&journal.convergence_id);
    let evidence = json!({
        "source": { "direction": plan.source.direction, "commit": journal.source_commit },
        "projections": local_axes["projections"],
        "registry_operation": { "state": "recorded", "operation_id": operation_id },
        "visibility": local_axes["visibility"],
        "remote": {
            "state": if matches!(plan.remote, crate::core::convergence::RemotePolicy::NotRequested) {
                "not_requested"
            } else {
                "pending_push"
            },
        },
        "recovery": { "state": "journaled", "journal_phase": "committing_registry" },
    });
    let payload = json!({
        "convergence_id": journal.convergence_id,
        "plan_id": journal.plan_id,
        "plan_digest": journal.plan_digest,
        "idempotency_binding_digest": journal.idempotency_binding_digest,
        "skill": journal.skill,
    });
    record_registry_operation_with_id(
        paths,
        &operation_id,
        "skill.converge",
        payload,
        evidence.clone(),
    )
    .map_err(map_registry_state)?;
    journal.aggregate_operation = paths
        .load_operations()
        .map_err(map_registry_state)?
        .into_iter()
        .find(|record| record.op_id == operation_id);
    journal.aggregate_checkpoint = Some(paths.load_checkpoint().map_err(map_registry_state)?);
    if journal.aggregate_operation.is_none() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "aggregate convergence operation disappeared after recording",
        ));
    }
    journal.aggregate_operation_id = Some(operation_id);
    journal.aggregate_evidence = Some(evidence);
    save_journal(journal_path, journal)
}

pub(super) fn record_source_only(
    plan: &SkillConvergencePlan,
    local_axes: &Value,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    journal.aggregate_operation_id = None;
    journal.aggregate_evidence = Some(json!({
        "source": { "direction": plan.source.direction, "commit": journal.source_commit },
        "projections": local_axes["projections"],
        "registry_operation": { "state": "not_applicable", "operation_id": null },
        "visibility": local_axes["visibility"],
        "remote": {
            "state": if matches!(plan.remote, crate::core::convergence::RemotePolicy::NotRequested) {
                "not_requested"
            } else {
                "pending_push"
            },
        },
        "recovery": { "state": "journaled", "journal_phase": "committing_registry" },
    }));
    save_journal(journal_path, journal)
}

pub(super) fn result(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    registry_commit: Option<String>,
) -> Value {
    json!({
        "convergence_id": journal.convergence_id,
        "plan_digest": journal.plan_digest,
        "idempotency_binding_digest": journal.idempotency_binding_digest,
        "skill": plan.skill,
        "source_commit": journal.source_commit,
        "registry_commit": registry_commit,
        "projection_instances": plan.projections.iter().map(|item| item.instance_id.clone()).collect::<Vec<_>>(),
        "aggregate_operation_id": journal.aggregate_operation_id,
        "evidence": journal.aggregate_evidence,
    })
}

pub(super) fn identity_is_valid(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    key_digest: &str,
    binding_digest: &str,
) -> bool {
    journal.plan_digest == plan.plan_digest
        && journal.idempotency_key_digest == key_digest
        && journal.idempotency_binding_digest == binding_digest
        && journal
            .convergence_id
            .strip_prefix("conv_")
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub(super) fn plan_identity_is_valid(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> bool {
    journal.plan_digest == plan.plan_digest
        && journal
            .convergence_id
            .strip_prefix("conv_")
            .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub(super) fn registry_commit_is_audit_only(journal: &TransactionJournal) -> bool {
    journal.aggregate_operation_id.is_some()
        && journal
            .expected_projections
            .as_ref()
            .is_some_and(|expected| {
                serde_json::to_value(expected).ok()
                    == serde_json::to_value(&journal.original_projections).ok()
            })
}

pub(super) fn live_is_valid(
    paths: &RegistryStatePaths,
    journal: &TransactionJournal,
) -> std::result::Result<bool, CommandFailure> {
    let Some(operation_id) = journal.aggregate_operation_id.as_deref() else {
        return Ok(false);
    };
    let Some(evidence) = journal.aggregate_evidence.as_ref() else {
        return Ok(false);
    };
    let operation = paths
        .load_operations()
        .map_err(map_registry_state)?
        .into_iter()
        .find(|record| record.op_id == operation_id);
    let checkpoint = paths.load_checkpoint().map_err(map_registry_state)?;
    let exact_operation = journal.aggregate_operation.as_ref();
    let exact_checkpoint = journal.aggregate_checkpoint.as_ref();
    Ok(operation.as_ref().is_some_and(|record| {
        record.intent == "skill.converge"
            && record.status == "succeeded"
            && !record.ack
            && record.last_error.is_none()
            && record.effects == *evidence
            && record.payload["convergence_id"] == json!(journal.convergence_id)
            && record.payload["plan_id"] == json!(journal.plan_id)
            && record.payload["plan_digest"] == json!(journal.plan_digest)
            && record.payload["idempotency_binding_digest"]
                == json!(journal.idempotency_binding_digest)
    }) && operation
        .as_ref()
        .zip(exact_operation)
        .is_some_and(|(actual, expected)| {
            serde_json::to_value(actual).ok() == serde_json::to_value(expected).ok()
        })
        && exact_checkpoint.is_some_and(|expected| {
            serde_json::to_value(&checkpoint).ok() == serde_json::to_value(expected).ok()
        })
        && checkpoint.last_scanned_op_id.as_deref() == Some(operation_id))
}
