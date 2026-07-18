use super::*;
use crate::commands::file_ops::copy_skill_tree_preserving_symlinks;
use crate::commands::provenance::convergence_input_tree_digest;

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
        let original = (effect.effect == "refresh")
            .then(|| convergence_projection_fingerprint(Path::new(&projection.materialized_path)))
            .transpose()?;
        if let (Some(backup), Some(fingerprint)) = (projection.backup.as_mut(), original.as_ref()) {
            backup["fingerprint"] = json!(fingerprint);
        }
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
        if original != live {
            return Err(stale(
                "projection changed while recording rollback evidence",
                "PLAN_PROJECTION_DRIFT",
            ));
        }
    }
    maybe_skill_fault("convergence_during_backup_preparation")?;
    let selected_source = selected_source_path(app, plan)?;
    if let Some(staging) = journal.source_staging.as_deref() {
        validate_selected_source(plan, &selected_source)?;
        reserve_owned_dir(
            Path::new(staging).parent().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "source stage has no owner")
            })?,
            &journal.plan_id,
            journal.source_owner_proof.as_deref().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "source owner proof is absent")
            })?,
        )?;
        copy_skill_tree_preserving_symlinks(&selected_source, Path::new(staging))
            .map_err(map_io)?;
        validate_selected_source(plan, Path::new(staging))?;
        journal.source_activated_fingerprint =
            Some(convergence_projection_fingerprint(Path::new(staging))?);
    }
    let projection_source = journal
        .source_staging
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or(selected_source);
    prepare_projection_stages_from(app, plan, "", journal, &projection_source)
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
        if effect.effect == "refresh"
            && artifact
                .backup
                .as_ref()
                .is_none_or(|backup| backup["fingerprint"].is_null())
        {
            artifact.backup.as_mut().expect("refresh backup")["fingerprint"] = json!(
                convergence_projection_fingerprint(Path::new(&artifact.materialized_path))?
            );
        }
        let materialized_path = Path::new(&artifact.materialized_path);
        let safe_symlink_noop = effect.effect == "refresh"
            && effect.method == "symlink"
            && projection_path_is_safe_symlink(materialized_path, &app.ctx.skill_path(&plan.skill));
        if safe_symlink_noop {
            artifact.activated_fingerprint =
                Some(convergence_projection_fingerprint(materialized_path)?);
            continue;
        }
        let staging_owner = Path::new(&artifact.staging_owner);
        fs::create_dir_all(staging_owner.parent().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "projection stage has no target root",
            )
        })?)
        .map_err(map_io)?;
        reserve_owned_dir(staging_owner, &journal.plan_id, &artifact.owner_proof)?;
        let input = projection_input(&snapshot, plan, effect, request_id)?;
        let stage_source = if effect.method == "symlink" {
            app.ctx.skill_path(&plan.skill)
        } else {
            source.to_path_buf()
        };
        prepare_convergence_projection(
            &app.ctx,
            &input,
            &stage_source,
            Path::new(&artifact.staging_path),
        )?;
        artifact.activated_fingerprint = Some(convergence_projection_fingerprint(Path::new(
            &artifact.staging_path,
        ))?);
    }
    Ok(())
}

fn validate_selected_source(
    plan: &SkillConvergencePlan,
    source: &Path,
) -> std::result::Result<(), CommandFailure> {
    let selected_method = plan
        .input
        .selected_projection_instance
        .as_deref()
        .and_then(|instance| {
            plan.projections
                .iter()
                .find(|effect| effect.instance_id == instance)
                .map(|effect| effect.method.as_str())
        });
    let digest = convergence_input_tree_digest(source, selected_method == Some("materialize"))
        .map_err(map_io)?;
    if digest != plan.input.selected_input_tree_digest {
        return Err(stale(
            "selected projection changed immediately before source staging",
            "PLAN_PROJECTION_DRIFT",
        ));
    }
    Ok(())
}
