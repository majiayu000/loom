use super::recovery_evidence::validate_mutated_surfaces;
use super::*;

pub(super) fn rollback_journal(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Err(err) = validate_mutated_surfaces(app, paths, plan, journal) {
        push_rollback_error(
            &mut errors,
            "validate_live_surfaces_before_rollback",
            err.message,
        );
        return errors;
    }
    if plan.registry.initialized
        && let Err(err) = paths.save_projections(&journal.original_projections)
    {
        push_rollback_error(&mut errors, "restore_registry_projections", err);
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_registry_restore") {
        return errors;
    }
    for projection in journal
        .projections
        .iter()
        .take(journal.installed_projections)
        .rev()
    {
        if let Err(err) = restore_projection_from_evidence(projection, &journal.plan_id) {
            push_rollback_error(&mut errors, "restore_projection_from_evidence", err.message);
        }
    }
    if rollback_fault(
        &mut errors,
        "convergence_interrupt_after_projection_restore",
    ) {
        return errors;
    }
    if journal.source_backup.is_some()
        && let Err(err) = restore_source_from_evidence(app, plan, journal)
    {
        push_rollback_error(&mut errors, "restore_source_path", err.message);
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_source_restore") {
        return errors;
    }
    if let Err(err) = restore_head_if_owned(app, journal) {
        push_rollback_error(&mut errors, "restore_head", err.message);
        return errors;
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_reset")
        || rollback_fault(&mut errors, "convergence_interrupt_before_index_restore")
    {
        return errors;
    }
    if let Err(err) = restore_index_if_owned(app, journal) {
        push_rollback_error(&mut errors, "restore_git_index", err.message);
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_index_restore") {
        return errors;
    }
    errors
}

fn restore_head_if_owned(
    app: &App,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let live = gitops::head(&app.ctx).map_err(map_git)?;
    if live == journal.previous_head {
        return Ok(());
    }
    if journal.rollback_head.as_deref() != Some(live.as_str()) {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "HEAD changed after rollback evidence was captured",
        ));
    }
    gitops::move_head_if_unchanged(&app.ctx, &journal.previous_head, &live).map_err(map_git)
}

fn restore_index_if_owned(
    app: &App,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let original = journal.index_backup_digest.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction Git index backup digest is missing",
        )
    })?;
    let live = active_index_digest(app)?;
    if live == original {
        return Ok(());
    }
    let rollback = journal.rollback_index_digest.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "rollback Git index digest is missing",
        )
    })?;
    if live != rollback {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "Git index changed after rollback evidence was captured",
        ));
    }
    gitops::install_prepared_index_with_guard(&app.ctx, Path::new(&journal.index_backup), |_| {
        let active = active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
        if active != rollback {
            return Err(anyhow::anyhow!(
                "active Git index changed before rollback installation"
            ));
        }
        let head = gitops::head(&app.ctx)?;
        if head != journal.previous_head {
            return Err(anyhow::anyhow!(
                "HEAD changed before rollback index installation"
            ));
        }
        Ok(())
    })
    .map_err(map_git)
}

fn rollback_fault(errors: &mut Vec<Value>, fault: &str) -> bool {
    if std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref() == Some(fault) {
        push_rollback_error(errors, "rollback_fault_injection", fault);
        true
    } else {
        false
    }
}

pub(super) fn finish_transaction(journal: &TransactionJournal) -> Vec<Value> {
    let mut errors = validate_transaction_artifacts(journal);
    if !errors.is_empty() {
        return errors;
    }
    for (index, projection) in journal.projections.iter().enumerate() {
        cleanup_owned_dir(
            Path::new(&projection.staging_owner),
            &journal.plan_id,
            &projection.owner_proof,
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
