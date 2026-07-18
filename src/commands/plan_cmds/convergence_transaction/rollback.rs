use super::*;

pub(super) fn rollback_journal(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
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
