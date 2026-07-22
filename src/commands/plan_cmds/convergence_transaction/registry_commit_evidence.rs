use super::recovery_evidence::corrupt;
use super::*;

const REGISTRY_PATH: &str = "state/registry/projections.json";
const LEGACY_OPERATIONS_PATH: &str = "state/registry/ops/operations.jsonl";
const LEGACY_CHECKPOINT_PATH: &str = "state/registry/ops/checkpoint.json";

pub(super) fn verify_registry_commit(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    head: &str,
    source_head: &str,
) -> std::result::Result<(), CommandFailure> {
    let legacy_audit = legacy_audit_evidence_is_complete(journal)?;
    super::registry_recovery::verify_commit(
        app,
        head,
        source_head,
        &format!("skill({}): record convergence projections", plan.skill),
        |path| {
            path == REGISTRY_PATH
                || (legacy_audit && matches!(path, LEGACY_OPERATIONS_PATH | LEGACY_CHECKPOINT_PATH))
        },
    )?;
    let expected = journal
        .expected_projections
        .as_ref()
        .ok_or_else(|| corrupt("missing expected projections"))?;
    let raw = gitops::run_git(&app.ctx, &["show", &format!("{head}:{REGISTRY_PATH}")])
        .map_err(map_git)?;
    let committed: RegistryProjectionsFile = serde_json::from_str(&raw)
        .map_err(|_| corrupt("registry commit projections are invalid"))?;
    if committed != *expected {
        return Err(super::registry_recovery::recovery_stale(
            "registry commit tree differs from transaction evidence",
        ));
    }
    if legacy_audit {
        verify_committed_legacy_audit(app, journal, head, source_head)?;
    }
    if matches!(
        journal.phase,
        TransactionPhase::CommittingRegistry | TransactionPhase::CommittedCleanupPending
    ) && journal.registry_commit.as_deref() == Some(head)
        && journal.registry_staged_index_digest.is_some()
    {
        return Ok(());
    }
    super::recovery_evidence::require_clean_path(app, REGISTRY_PATH)?;
    if legacy_audit {
        super::recovery_evidence::require_clean_path(app, LEGACY_OPERATIONS_PATH)?;
        super::recovery_evidence::require_clean_path(app, LEGACY_CHECKPOINT_PATH)?;
    }
    Ok(())
}

pub(super) fn legacy_audit_evidence_is_complete(
    journal: &TransactionJournal,
) -> std::result::Result<bool, CommandFailure> {
    let aggregate_present = [
        journal.aggregate_operation_id.is_some(),
        journal.aggregate_operation.is_some(),
        journal.aggregate_checkpoint.is_some(),
    ];
    if aggregate_present.iter().all(|value| !value) {
        return Ok(false);
    }
    if aggregate_present.iter().all(|value| *value)
        && journal.aggregate_evidence.is_some()
        && journal.original_operations.is_some()
        && journal.original_checkpoint.is_some()
    {
        return Ok(true);
    }
    Err(corrupt("legacy convergence audit evidence is incomplete"))
}

fn verify_committed_legacy_audit(
    app: &App,
    journal: &TransactionJournal,
    head: &str,
    source_head: &str,
) -> std::result::Result<(), CommandFailure> {
    let operation_id = journal
        .aggregate_operation_id
        .as_deref()
        .ok_or_else(|| corrupt("missing legacy aggregate operation id"))?;
    let evidence = journal
        .aggregate_evidence
        .as_ref()
        .ok_or_else(|| corrupt("missing legacy aggregate evidence"))?;
    let operation = journal
        .aggregate_operation
        .as_ref()
        .ok_or_else(|| corrupt("missing legacy aggregate operation"))?;
    let checkpoint = journal
        .aggregate_checkpoint
        .as_ref()
        .ok_or_else(|| corrupt("missing legacy aggregate checkpoint"))?;
    let original_operations = journal
        .original_operations
        .as_ref()
        .ok_or_else(|| corrupt("missing legacy original operations"))?;
    let original_checkpoint = journal
        .original_checkpoint
        .as_ref()
        .ok_or_else(|| corrupt("missing legacy original checkpoint"))?;

    let operation_identity_matches = operation.op_id == operation_id
        && operation.intent == "skill.converge"
        && operation.status == "succeeded"
        && !operation.ack
        && operation.last_error.is_none()
        && operation.effects == *evidence
        && operation.payload["convergence_id"] == json!(journal.convergence_id)
        && operation.payload["plan_id"] == json!(journal.plan_id)
        && operation.payload["plan_digest"] == json!(journal.plan_digest)
        && operation.payload["idempotency_binding_digest"]
            == json!(journal.idempotency_binding_digest)
        && checkpoint.last_scanned_op_id.as_deref() == Some(operation_id);
    if !operation_identity_matches {
        return Err(corrupt("legacy aggregate operation identity is invalid"));
    }

    let parent_operations = committed_operations(app, source_head)?;
    let committed_operations = committed_operations(app, head)?;
    if !same_value(&parent_operations, original_operations)?
        || committed_operations.len() != original_operations.len() + 1
        || !same_value(
            &committed_operations[..original_operations.len()],
            original_operations,
        )?
        || !same_value(&committed_operations[original_operations.len()], operation)?
    {
        return Err(super::registry_recovery::recovery_stale(
            "legacy registry commit operations differ from transaction evidence",
        ));
    }

    let parent_checkpoint = committed_checkpoint(app, source_head)?;
    let committed_checkpoint = committed_checkpoint(app, head)?;
    if !same_value(&parent_checkpoint, original_checkpoint)?
        || !same_value(&committed_checkpoint, checkpoint)?
    {
        return Err(super::registry_recovery::recovery_stale(
            "legacy registry commit checkpoint differs from transaction evidence",
        ));
    }
    Ok(())
}

fn committed_operations(
    app: &App,
    head: &str,
) -> std::result::Result<Vec<crate::state_model::RegistryOperationRecord>, CommandFailure> {
    let raw = gitops::run_git(
        &app.ctx,
        &["show", &format!("{head}:{LEGACY_OPERATIONS_PATH}")],
    )
    .map_err(map_git)?;
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| corrupt("legacy registry commit operations are invalid"))
}

fn committed_checkpoint(
    app: &App,
    head: &str,
) -> std::result::Result<crate::state_model::RegistryOpsCheckpoint, CommandFailure> {
    let raw = gitops::run_git(
        &app.ctx,
        &["show", &format!("{head}:{LEGACY_CHECKPOINT_PATH}")],
    )
    .map_err(map_git)?;
    serde_json::from_str(&raw).map_err(|_| corrupt("legacy registry commit checkpoint is invalid"))
}

fn same_value<T: serde::Serialize + ?Sized>(
    left: &T,
    right: &T,
) -> std::result::Result<bool, CommandFailure> {
    Ok(serde_json::to_value(left).map_err(map_io)?
        == serde_json::to_value(right).map_err(map_io)?)
}
