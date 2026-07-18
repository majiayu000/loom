use super::*;

pub(super) fn declared_backup(
    path: &Path,
    backup_path: &Path,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(map_io(err)),
    };
    let kind = if metadata.file_type().is_symlink() {
        "symlink"
    } else if metadata.is_dir() {
        "dir"
    } else {
        "file"
    };
    Ok(Some(json!({
        "kind": kind,
        "original_path": path.display().to_string(),
        "backup_path": backup_path.display().to_string(),
    })))
}

#[inline(never)]
pub(super) fn prepare_transaction_artifacts(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    reserve_owned_dir(
        Path::new(&journal.artifact_root),
        &journal.plan_id,
        &journal.artifact_owner_proof,
    )?;
    gitops::snapshot_index_to(&app.ctx, Path::new(&journal.index_backup)).map_err(map_git)?;
    journal.index_backup_digest = Some(file_digest(Path::new(&journal.index_backup))?);
    save_journal(journal_path, journal)?;
    if let Some(backup) = journal.source_backup.as_ref() {
        let source = app.ctx.skill_path(&plan.skill);
        if skill_tree_digest(&source).map_err(map_io)? != plan.source.tree_digest {
            return Err(stale(
                "source changed before rollback backup",
                "PLAN_SOURCE_DRIFT",
            ));
        }
        create_declared_path_backup(&source, backup).map_err(map_io)?;
        validate_tree_backup(backup, &plan.source.tree_digest, None, None)?;
        if skill_tree_digest(&source).map_err(map_io)? != plan.source.tree_digest {
            return Err(stale(
                "source changed while recording rollback evidence",
                "PLAN_SOURCE_DRIFT",
            ));
        }
    }
    for (effect, projection) in plan.projections.iter().zip(&mut journal.projections) {
        validate_projection_guard(app, plan, effect)?;
        projection.original_fingerprint = (effect.effect == "refresh")
            .then(|| convergence_projection_fingerprint(Path::new(&projection.materialized_path)))
            .transpose()?;
        if let Some(backup) = projection.backup.as_ref() {
            create_declared_path_backup(Path::new(&projection.materialized_path), backup)
                .map_err(map_io)?;
            validate_tree_backup(
                backup,
                effect
                    .materialized_tree_digest
                    .as_deref()
                    .unwrap_or_default(),
                (effect.method == "symlink").then(|| app.ctx.skill_path(&plan.skill)),
                Some(&effect.method),
            )?;
        }
        validate_projection_guard(app, plan, effect)?;
        let live = (effect.effect == "refresh")
            .then(|| convergence_projection_fingerprint(Path::new(&projection.materialized_path)))
            .transpose()?;
        if projection.original_fingerprint != live {
            return Err(stale(
                "projection changed while recording rollback evidence",
                "PLAN_PROJECTION_DRIFT",
            ));
        }
    }
    maybe_skill_fault("convergence_during_backup_preparation")?;
    let selected_source = selected_source_path(app, plan)?;
    if let Some(staging) = journal.source_staging.as_deref() {
        reserve_owned_dir(
            Path::new(staging)
                .parent()
                .ok_or_else(|| state_corrupt("source stage has no owner"))?,
            &journal.plan_id,
            journal
                .source_owner_proof
                .as_deref()
                .ok_or_else(|| state_corrupt("source owner proof is absent"))?,
        )?;
        project_skill_to_target(&selected_source, Path::new(staging), ProjectionMethod::Copy)
            .map_err(map_io)?;
        journal.source_activated_fingerprint =
            Some(convergence_projection_fingerprint(Path::new(staging))?);
    }
    prepare_projection_stages_from(app, plan, "", journal, &selected_source)
}

pub(super) fn prepare_projection_stages(
    app: &App,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    prepare_projection_stages_from(
        app,
        plan,
        request_id,
        journal,
        &app.ctx.skill_path(&plan.skill),
    )
}

#[inline(never)]
fn prepare_projection_stages_from(
    app: &App,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal: &mut TransactionJournal,
    source: &Path,
) -> std::result::Result<(), CommandFailure> {
    if plan.projections.is_empty() {
        return Ok(());
    }
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
    for (effect, artifact) in plan.projections.iter().zip(journal.projections.iter_mut()) {
        validate_projection_guard(app, plan, effect)?;
        if effect.effect == "refresh" && artifact.original_fingerprint.is_none() {
            artifact.original_fingerprint = Some(convergence_projection_fingerprint(Path::new(
                &artifact.materialized_path,
            ))?);
        }
        reserve_owned_dir(
            Path::new(&artifact.staging_owner),
            &journal.plan_id,
            &artifact.owner_proof,
        )?;
        let input = projection_input(&snapshot, plan, effect, request_id)?;
        prepare_convergence_projection(
            &app.ctx,
            &input,
            source,
            Path::new(&artifact.staging_path),
        )?;
        artifact.activated_fingerprint = Some(convergence_projection_fingerprint(Path::new(
            &artifact.staging_path,
        ))?);
    }
    Ok(())
}
