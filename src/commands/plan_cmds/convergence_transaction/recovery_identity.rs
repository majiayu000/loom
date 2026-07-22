use super::*;

pub(super) fn adopt_journal_identity(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    identity: &mut super::super::ConvergenceApplyIdentity,
) -> std::result::Result<(), CommandFailure> {
    if journal.phase == TransactionPhase::RolledBackArtifactsRetained {
        return Ok(());
    }
    let mismatch = || {
        plan_failure(
            ErrorCode::DependencyConflict,
            "convergence journal is bound to a different idempotency identity",
            "IDEMPOTENCY_BINDING_MISMATCH",
            false,
            vec!["retry with the idempotency key that owns this convergence journal".to_string()],
            None,
        )
    };
    let Some(convergence_id) = journal.convergence_id.as_deref().filter(|value| {
        value
            .strip_prefix("conv_")
            .is_some_and(|suffix| !suffix.is_empty())
    }) else {
        return Err(mismatch());
    };
    let Some(key_digest) = journal.idempotency_key_digest.as_deref() else {
        return Err(mismatch());
    };
    let Some(binding_digest) = journal.idempotency_binding_digest.as_deref() else {
        return Err(mismatch());
    };
    let plan_identity_matches = journal.plan_id == plan.plan_id
        && journal.plan_digest.as_deref() == Some(identity.plan_digest.as_str());
    let exact_identity = plan_identity_matches
        && key_digest == identity.key_digest
        && binding_digest == identity.binding_digest;
    let terminal_replay = matches!(
        journal.phase,
        TransactionPhase::CommittedCleanupPending | TransactionPhase::CommittedArtifactsRetained
    ) || journal.registry_commit.is_some()
        || (journal.phase == TransactionPhase::CommittingRegistry
            && super::registry_commit::durable_registry_noop(journal));
    if !exact_identity && !(terminal_replay && plan_identity_matches) {
        return Err(mismatch());
    }
    identity.convergence_id = convergence_id.to_string();
    identity.key_digest = key_digest.to_string();
    identity.binding_digest = binding_digest.to_string();
    Ok(())
}
