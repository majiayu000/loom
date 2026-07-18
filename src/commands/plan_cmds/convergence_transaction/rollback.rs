use super::*;

pub(super) fn rollback_journal(
    app: &App,
    plan: &SkillConvergencePlan,
    paths: &RegistryStatePaths,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
    let mut errors = validate_batch_preflight(app, plan, journal, BatchOperation::Rollback);
    if !errors.is_empty() {
        return errors;
    }
    if let Err(err) = paths.save_projections(&journal.original_projections) {
        push_rollback_error(&mut errors, "restore_registry_projections", err);
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_registry_restore") {
        return errors;
    }
    for index in (0..journal.installed_projections).rev() {
        if let Some(artifact) = journal.projections[index].rollback.as_mut() {
            if let Err(err) = artifact.prepare_rollback() {
                push_rollback_error(&mut errors, "prepare_projection_rollback", err.message);
                return errors;
            }
            journal.projections[index].state = ProjectionTransactionState::RollbackCleanupPending;
            if let Err(err) = save_journal(journal_path, journal) {
                push_rollback_error(&mut errors, "persist_projection_rollback", err.message);
                return errors;
            }
            if let Some(artifact) = journal.projections[index].rollback.as_mut()
                && let Err(err) = artifact.cleanup_pending()
            {
                push_rollback_error(&mut errors, "cleanup_projection_rollback", err.message);
                return errors;
            }
            journal.projections[index].rollback = None;
            journal.projections[index].state = ProjectionTransactionState::RolledBack;
            if let Err(err) = save_journal(journal_path, journal) {
                push_rollback_error(&mut errors, "persist_projection_cleanup", err.message);
                return errors;
            }
            if index + 1 == journal.installed_projections
                && rollback_fault(
                    &mut errors,
                    "convergence_interrupt_after_first_projection_rollback",
                )
            {
                return errors;
            }
        }
        journal.projections[index].state = ProjectionTransactionState::RolledBack;
    }
    for index in journal.installed_projections..journal.projections.len() {
        if let Some(prepared) = journal.projections[index].prepared.take()
            && let Err(err) =
                discard_prepared_projection(PreparedProjection::from_durable_artifact(prepared))
        {
            push_rollback_error(&mut errors, "discard_prepared_projection", err.message);
            return errors;
        }
        journal.projections[index].state = ProjectionTransactionState::RolledBack;
    }
    if rollback_fault(
        &mut errors,
        "convergence_interrupt_after_projection_restore",
    ) {
        return errors;
    }
    if let (Some(backup), Some(staging)) = (
        journal.source_backup.as_ref(),
        journal.source_staging.as_deref(),
    ) && let Err(err) = restore_backup_atomically(
        &app.ctx.skill_path(&journal.skill),
        backup,
        Path::new(staging),
        &journal.plan_id,
        journal.source_owner_proof.as_deref().unwrap_or_default(),
    ) {
        push_rollback_error(&mut errors, "restore_source_path", err.message);
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_source_restore") {
        return errors;
    }
    match gitops::run_git_allow_failure(&app.ctx, &["reset", "--soft", &journal.previous_head]) {
        Ok(output) if output.status.success() => {}
        Ok(output) => push_rollback_error(
            &mut errors,
            "restore_head",
            String::from_utf8_lossy(&output.stderr).trim(),
        ),
        Err(err) => push_rollback_error(&mut errors, "restore_head", err),
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_reset")
        || rollback_fault(&mut errors, "convergence_interrupt_before_index_restore")
    {
        return errors;
    }
    if let Err(err) = gitops::restore_index_from_backup(&app.ctx, Path::new(&journal.index_backup))
    {
        push_rollback_error(&mut errors, "restore_git_index", err);
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_index_restore") {
        return errors;
    }
    errors
}

fn rollback_fault(errors: &mut Vec<Value>, fault: &str) -> bool {
    if std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref() == Some(fault) {
        push_rollback_error(errors, "rollback_fault_injection", fault);
        true
    } else {
        false
    }
}

pub(super) fn finish_transaction(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
    let mut errors = validate_batch_preflight(app, plan, journal, BatchOperation::Finalize);
    if !errors.is_empty() {
        return errors;
    }
    for index in 0..journal.projections.len() {
        if let Some(artifact) = journal.projections[index].rollback.as_mut() {
            if let Err(err) = artifact.prepare_finalize() {
                push_rollback_error(&mut errors, "prepare_projection_finalize", err.message);
                return errors;
            }
            journal.projections[index].state = ProjectionTransactionState::FinalizeCleanupPending;
            if let Err(err) = save_journal(journal_path, journal) {
                push_rollback_error(&mut errors, "persist_projection_finalize", err.message);
                return errors;
            }
            if let Some(artifact) = journal.projections[index].rollback.as_mut()
                && matches!(artifact, ProjectionRollbackArtifact::PendingCleanup { .. })
                && let Err(err) = artifact.cleanup_pending()
            {
                push_rollback_error(&mut errors, "cleanup_projection_finalize", err.message);
                return errors;
            }
            journal.projections[index].rollback = None;
            journal.projections[index].state = ProjectionTransactionState::Finalized;
            if let Err(err) = save_journal(journal_path, journal) {
                push_rollback_error(&mut errors, "persist_projection_cleanup", err.message);
                return errors;
            }
            if index == 0
                && std::env::var("LOOM_CLEANUP_FAULT_INJECT").ok().as_deref()
                    == Some("convergence_interrupt_after_first_projection_finalize")
            {
                push_rollback_error(
                    &mut errors,
                    "cleanup_transaction_backups",
                    "fault injected after first projection finalize",
                );
                return errors;
            }
        }
        let staging_owner = journal.projections[index].staging_owner.clone();
        let owner_proof = journal.projections[index].owner_proof.clone();
        cleanup_owned_dir(
            Path::new(&staging_owner),
            &journal.plan_id,
            &owner_proof,
            &mut errors,
        );
        if !errors.is_empty() {
            return errors;
        }
        if index == 0
            && (std::env::var("LOOM_FAULT_INJECT").ok().as_deref()
                == Some("convergence_interrupt_during_cleanup")
                || std::env::var("LOOM_CLEANUP_FAULT_INJECT").ok().as_deref()
                    == Some("convergence_interrupt_during_cleanup"))
        {
            push_rollback_error(
                &mut errors,
                "cleanup_transaction_backups",
                "fault injected during committed cleanup",
            );
            return errors;
        }
    }
    if let Some(path) = journal.source_staging.as_deref()
        && let Some(owner) = Path::new(path).parent()
        && let Some(proof) = journal.source_owner_proof.as_deref()
    {
        cleanup_owned_dir(owner, &journal.plan_id, proof, &mut errors);
        if !errors.is_empty() {
            return errors;
        }
    }
    cleanup_owned_dir(
        Path::new(&journal.artifact_root),
        &journal.plan_id,
        &journal.artifact_owner_proof,
        &mut errors,
    );
    errors
}

#[derive(Clone, Copy)]
enum BatchOperation {
    Rollback,
    Finalize,
}

fn validate_batch_preflight(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    operation: BatchOperation,
) -> Vec<Value> {
    let mut errors = validate_transaction_artifacts(journal);
    let selected_source = match selected_source_path(app, plan) {
        Ok(path) => path,
        Err(err) => {
            push_rollback_error(&mut errors, "resolve_projection_source", err.message);
            return errors;
        }
    };
    if let Err(err) = validate_projection_transaction(plan, journal, &selected_source) {
        push_rollback_error(&mut errors, "validate_projection_transaction", err.message);
    }
    for projection in &journal.projections {
        if let Some(prepared) = projection.prepared.as_ref()
            && let Err(err) = validate_prepared_projection_artifact(prepared)
        {
            push_rollback_error(&mut errors, "validate_prepared_projection", err.message);
        }
        if let Some(artifact) = projection.rollback.as_ref() {
            let result = match operation {
                BatchOperation::Rollback => {
                    validate_projection_rollback_artifact_for_rollback(artifact)
                }
                BatchOperation::Finalize => {
                    validate_projection_rollback_artifact_for_finalize(artifact)
                }
            };
            if let Err(err) = result {
                push_rollback_error(&mut errors, "validate_projection_rollback", err.message);
            }
        }
    }
    errors
}
