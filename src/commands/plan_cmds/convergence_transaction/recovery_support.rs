use super::*;

pub(super) fn validate_projection_guard(
    app: &App,
    plan: &SkillConvergencePlan,
    effect: &crate::core::convergence::ProjectionEffectPlan,
) -> std::result::Result<(), CommandFailure> {
    let path = Path::new(&effect.materialized_path);
    if let Some(expected) = effect.materialized_tree_digest.as_deref() {
        let live = skill_tree_digest(path).map_err(map_io)?;
        if live == expected {
            return Ok(());
        }
    } else {
        match fs::symlink_metadata(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Ok(metadata)
                if effect.method == "symlink"
                    && metadata.file_type().is_symlink()
                    && projection_path_is_safe_symlink(path, &app.ctx.skill_path(&plan.skill)) =>
            {
                return Ok(());
            }
            Err(err) => return Err(map_io(err)),
            Ok(_) => {}
        }
    }
    Err(stale(
        "projection bytes or path kind changed after planning",
        "PLAN_PROJECTION_DRIFT",
    ))
}

pub(super) fn apply_output(
    plan: &SkillConvergencePlan,
    cursor: usize,
    key_digest: &str,
    output: Value,
) -> Value {
    json!({
        "protocol_version": PLAN_PROTOCOL_VERSION,
        "schema_version": SCHEMA_VERSION,
        "plan_id": plan.plan_id,
        "idempotency_key_digest": key_digest,
        "idempotent_replay": false,
        "plan_event_cursor": cursor,
        "applied": output,
        "recovery": { "rollback_supported": true },
    })
}

pub(super) fn source_is_committed(journal: &TransactionJournal) -> bool {
    journal.source_commit.is_some()
}

pub(super) fn committed_result(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<Value, CommandFailure> {
    Ok(json!({
        "skill": plan.skill,
        "source_commit": journal.source_commit,
        "registry_commit": gitops::head(&app.ctx).map_err(map_git)?,
        "projection_instances": plan.projections.iter().map(|effect| effect.instance_id.clone()).collect::<Vec<_>>(),
    }))
}

pub(super) fn restore_projections_for_resume(
    paths: &RegistryStatePaths,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let mut errors = Vec::new();
    if let Err(err) = paths.save_projections(&journal.original_projections) {
        push_rollback_error(&mut errors, "restore_registry_projections", err);
    }
    for projection in journal.projections.iter().rev() {
        errors.extend(rollback_convergence_projection(
            Path::new(&projection.materialized_path),
            projection.backup.as_ref(),
        ));
        if let Err(err) = remove_path_if_exists(Path::new(&projection.staging_path)) {
            push_rollback_error(&mut errors, "remove_projection_staging", err);
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

pub(super) fn cleanup_declared_artifacts(
    journal_path: &Path,
    journal: &TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    for projection in journal.projections.iter().rev() {
        cleanup_persistent_backup(projection.backup.as_ref(), &mut errors);
        if let Err(err) = remove_path_if_exists(Path::new(&projection.staging_path)) {
            push_rollback_error(&mut errors, "remove_projection_staging", err);
        }
    }
    cleanup_persistent_backup(journal.source_backup.as_ref(), &mut errors);
    if let Some(path) = journal.source_staging.as_deref()
        && let Err(err) = remove_path_if_exists(Path::new(path))
    {
        push_rollback_error(&mut errors, "remove_source_staging", err);
    }
    for (step, path) in [
        ("remove_index_snapshot", Path::new(&journal.index_backup)),
        ("remove_transaction_journal", journal_path),
    ] {
        if let Err(err) = remove_path_if_exists(path) {
            push_rollback_error(&mut errors, step, err);
        }
    }
    errors
}
