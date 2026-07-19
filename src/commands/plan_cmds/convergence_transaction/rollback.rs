use super::ownership_state::OwnershipAttemptState;
use super::recovery_evidence::validate_mutated_surfaces;
use super::registry_restore::restore_registry_projections_if_owned;
use super::*;

pub(super) fn handle_transaction_failure(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    failure: CommandFailure,
) -> std::result::Result<CommandFailure, CommandFailure> {
    if journal.phase == TransactionPhase::RolledBackArtifactsRetained {
        return Ok(failure);
    }
    let rollback_head = gitops::head(&app.ctx).map_err(map_git)?;
    if rollback_head != journal.previous_head
        && super::external_head::retire_uncommitted_noop_after_external_head(
            app,
            paths,
            plan,
            journal_path,
            journal,
        )?
    {
        return Ok(failure);
    }
    if rollback_head != journal.previous_head
        && journal.source_head.as_deref() != Some(rollback_head.as_str())
    {
        return Ok(failure.with_rollback_errors(vec![json!({
            "step": "capture_rollback_head",
            "message": "HEAD is neither old nor the recorded transaction head",
        })]));
    }
    journal.registry_commit = None;
    journal.registry_staged_index_digest = None;
    journal.rollback_head = Some(rollback_head);
    journal.rollback_index_digest = Some(active_index_digest(app)?);
    journal.phase = TransactionPhase::RollingBack;
    let mut errors = save_journal(journal_path, journal)
        .err()
        .map(|error| {
            vec![json!({
                "step": "persist_rolling_back",
                "message": error.message,
            })]
        })
        .unwrap_or_default();
    if errors.is_empty() {
        if let Err(error) = validate_rollback_evidence(app, plan, journal) {
            errors.push(json!({
                "step": "validate_rollback_evidence",
                "message": error.message,
            }));
        } else {
            errors = rollback_journal(app, paths, plan, journal_path, journal);
        }
    }
    if errors.is_empty()
        && std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref()
            == Some("convergence_interrupt_after_rollback")
    {
        return Ok(failure);
    }
    if errors.is_empty() {
        registry_commit::terminalize_registry_index_attempts(journal, false);
        journal.phase = TransactionPhase::RolledBackCleanupPending;
        if let Err(error) = save_journal(journal_path, journal) {
            errors.push(json!({
                "step": "persist_rolled_back_cleanup_pending",
                "message": error.message,
            }));
        }
    }
    if errors.is_empty() {
        errors.extend(finish_transaction(journal_path, journal));
    }
    Ok(failure.with_rollback_errors(errors))
}

pub(super) fn restore_registry_and_activated_projections(
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if plan.registry.initialized
        && let Err(error) = restore_registry_projections_if_owned(paths, journal)
    {
        push_rollback_error(&mut errors, "restore_registry_projections", error.message);
        return errors;
    }
    errors.extend(restore_activated_projections(journal_path, journal));
    errors
}

pub(super) fn restore_activated_projection_at(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    index: usize,
) -> std::result::Result<(), CommandFailure> {
    if journal.projections[index].restored_fingerprint.is_none()
        && let Some(fingerprint) =
            prepare_projection_restore_fingerprint(&journal.projections[index], &journal.plan_id)?
    {
        journal.projections[index].restored_fingerprint = Some(fingerprint);
        sync_installed_projection_count(journal);
        save_journal(journal_path, journal)?;
        maybe_skill_fault("convergence_interrupt_after_durable_projection_restore_intent")?;
        #[cfg(debug_assertions)]
        if std::env::var("LOOM_TEST_CONVERGENCE_RESTORE_WAL_INDEX")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            == Some(index)
        {
            maybe_skill_fault("convergence_interrupt_after_projection_restore_wal")?;
        }
    }
    restore_projection_from_evidence(&journal.projections[index], &journal.plan_id)?;
    #[cfg(debug_assertions)]
    if std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref()
        == Some("convergence_interrupt_after_projection_restore_exchange")
        && std::env::var("LOOM_TEST_CONVERGENCE_RESTORE_WAL_INDEX")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            == Some(index)
    {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            "fault injected after projection restore exchange",
        ));
    }
    journal.projections[index].mark_activated(false);
    sync_installed_projection_count(journal);
    Ok(())
}

fn sync_installed_projection_count(journal: &mut TransactionJournal) {
    journal.installed_projections = journal
        .projections
        .iter()
        .filter(|projection| projection.is_activated())
        .count();
}

pub(super) fn restore_activated_projections(
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    for index in (0..journal.projections.len()).rev() {
        if journal.projections[index].is_activated()
            && let Err(err) = restore_activated_projection_at(journal_path, journal, index)
        {
            push_rollback_error(
                &mut errors,
                "restore_projection_after_head_drift",
                err.message,
            );
        }
    }
    sync_installed_projection_count(journal);
    errors
}

pub(super) fn restore_projections_for_resume(
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let mut errors = validate_transaction_artifacts(journal);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "committed source recovery artifact validation failed",
        )
        .with_rollback_errors(errors));
    }
    if plan.registry.initialized
        && let Err(err) = restore_registry_projections_if_owned(paths, journal)
    {
        push_rollback_error(&mut errors, "restore_registry_projections", err.message);
    }
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "failed to prepare committed source recovery",
        )
        .with_rollback_errors(errors));
    }
    for index in (0..journal.projections.len()).rev() {
        if !journal.projections[index].is_activated() {
            continue;
        }
        if let Err(err) = restore_activated_projection_at(journal_path, journal, index) {
            push_rollback_error(&mut errors, "restore_projection_from_evidence", err.message);
        }
        if !errors.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "failed to prepare committed source recovery",
            )
            .with_rollback_errors(errors));
        }
    }
    for projection in journal.projections.iter().rev() {
        cleanup_owned_dir(
            Path::new(&projection.staging_owner),
            &journal.plan_id,
            &projection.owner_proof,
            &mut errors,
        );
        if !errors.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "failed to prepare committed source recovery",
            )
            .with_rollback_errors(errors));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "failed to prepare committed source recovery",
        )
        .with_rollback_errors(errors))
    }
}

pub(super) fn rollback_journal(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    errors.extend(validate_transaction_artifacts(journal));
    if !errors.is_empty() {
        return errors;
    }
    if let Err(err) = validate_mutated_surfaces(app, paths, plan, journal) {
        push_rollback_error(
            &mut errors,
            "validate_live_surfaces_before_rollback",
            err.message,
        );
        return errors;
    }
    if plan.registry.initialized
        && let Err(err) = restore_registry_projections_if_owned(paths, journal)
    {
        push_rollback_error(&mut errors, "restore_registry_projections", err.message);
        return errors;
    }
    if rollback_fault(&mut errors, "convergence_interrupt_after_registry_restore") {
        return errors;
    }
    errors.extend(restore_activated_projections(journal_path, journal));
    if !errors.is_empty() {
        return errors;
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
    gitops::install_prepared_index_with_guard(&app.ctx, Path::new(&journal.index_backup), &|_| {
        let active = active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
        if active != rollback {
            return Err(anyhow::anyhow!("active Git index changed"));
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

pub(super) fn finish_transaction(
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> Vec<Value> {
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
    if errors.is_empty() {
        let committed = journal.phase == TransactionPhase::CommittedCleanupPending;
        super::registry_commit::terminalize_registry_index_attempts(journal, committed);
        for attempt in &mut journal.ownership_attempts {
            attempt.state = match attempt.state {
                OwnershipAttemptState::Activated => OwnershipAttemptState::Retained,
                OwnershipAttemptState::Allocated | OwnershipAttemptState::Ready => {
                    OwnershipAttemptState::Abandoned
                }
                state => state,
            };
        }
        journal.phase = match journal.phase {
            TransactionPhase::CommittedCleanupPending => {
                TransactionPhase::CommittedArtifactsRetained
            }
            TransactionPhase::RolledBackCleanupPending => {
                TransactionPhase::RolledBackArtifactsRetained
            }
            _ => {
                push_rollback_error(
                    &mut errors,
                    "retain_transaction_artifacts",
                    "transaction is not in a cleanup-pending phase",
                );
                return errors;
            }
        };
        if let Err(err) = save_journal(journal_path, journal) {
            push_rollback_error(
                &mut errors,
                "persist_retained_transaction_artifacts",
                err.message,
            );
        }
    }
    errors
}
