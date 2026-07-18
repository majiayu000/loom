use super::ownership_state::OwnershipAttemptState;
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
    let backup_digest = match kind {
        "dir" => skill_tree_digest(path).map_err(map_io)?,
        "file" => file_digest(path)?,
        "symlink" => {
            let target = fs::read_link(path).map_err(map_io)?;
            digest_value(&json!({"target": target.display().to_string()}))?
        }
        _ => unreachable!("declared backup kind is exhaustive"),
    };
    Ok(Some(json!({
        "kind": kind,
        "original_path": path.display().to_string(),
        "backup_path": backup_path.display().to_string(),
        "backup_digest": backup_digest,
    })))
}

pub(super) fn prepare_transaction_artifacts(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let artifact_root = journal.artifact_root.clone();
    let artifact_proof = journal.artifact_owner_proof.clone();
    activate_owned_dir(
        journal_path,
        journal,
        Path::new(&artifact_root),
        &artifact_proof,
    )?;
    prepare_index_backup(app, journal_path, journal)?;
    if let Some(backup) = journal.source_backup.as_ref() {
        prepare_declared_backup(&app.ctx.skill_path(&plan.skill), backup)?;
    }
    for projection in &journal.projections {
        if let Some(backup) = projection.backup.as_ref() {
            prepare_declared_backup(Path::new(&projection.materialized_path), backup)?;
        }
    }
    maybe_skill_fault("convergence_during_backup_preparation")?;
    let selected_source = selected_source_path(app, plan)?;
    if let Some(staging) = journal.source_staging.clone() {
        validate_selected_source(plan, &selected_source)?;
        let owner = Path::new(&staging)
            .parent()
            .ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "source stage has no owner")
            })?
            .to_path_buf();
        let proof = journal.source_owner_proof.clone().ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "source owner proof is absent")
        })?;
        activate_owned_dir(journal_path, journal, &owner, &proof)?;
        copy_skill_tree_preserving_symlinks(&selected_source, Path::new(&staging))
            .map_err(map_io)?;
        validate_selected_source(plan, Path::new(&staging))?;
        journal.source_activated_fingerprint =
            Some(convergence_projection_fingerprint(Path::new(&staging))?);
    }
    let projection_source = journal
        .source_staging
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or(selected_source);
    prepare_projection_stages_from(app, plan, "", journal_path, journal, &projection_source)
}

fn prepare_index_backup(
    app: &App,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let backup = Path::new(&journal.index_backup);
    if backup.exists() {
        let actual = file_digest(backup)?;
        if let Some(expected) = journal.index_backup_digest.as_deref() {
            if actual != expected {
                return Err(CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    "existing transaction Git index backup does not match its journal digest",
                ));
            }
            return Ok(());
        }
        let live = active_index_digest(app)?;
        if actual != live {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "uncommitted transaction Git index backup does not match the locked Git index",
            ));
        }
        journal.index_backup_digest = Some(actual);
        save_journal(journal_path, journal)?;
        maybe_skill_fault("convergence_interrupt_after_index_snapshot_digest")?;
        return Ok(());
    }
    if journal.index_backup_digest.is_some() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "transaction Git index backup is missing despite a persisted digest",
        ));
    }
    maybe_skill_fault("convergence_interrupt_before_index_snapshot")?;
    gitops::snapshot_index_to(&app.ctx, backup).map_err(map_git)?;
    maybe_skill_fault("convergence_interrupt_after_index_snapshot")?;
    journal.index_backup_digest = Some(file_digest(backup)?);
    save_journal(journal_path, journal)?;
    maybe_skill_fault("convergence_interrupt_after_index_snapshot_digest")?;
    Ok(())
}

fn prepare_declared_backup(
    original: &Path,
    backup: &Value,
) -> std::result::Result<(), CommandFailure> {
    let backup_path = backup["backup_path"]
        .as_str()
        .map(Path::new)
        .ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "declared backup has no path")
        })?;
    let expected = backup["backup_digest"].as_str().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "declared backup has no persisted digest",
        )
    })?;
    if !backup_path.exists() {
        create_declared_path_backup(original, backup).map_err(map_io)?;
    }
    let actual = declared_backup_digest(backup_path, backup)?;
    if actual != expected {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "existing declared backup {} does not match its journal digest",
                backup_path.display()
            ),
        ));
    }
    Ok(())
}

fn declared_backup_digest(
    backup_path: &Path,
    backup: &Value,
) -> std::result::Result<String, CommandFailure> {
    match backup["kind"].as_str() {
        Some("dir") => skill_tree_digest(backup_path).map_err(map_io),
        Some("file") => file_digest(backup_path),
        Some("symlink") => {
            let raw = fs::read_to_string(backup_path.join("symlink.json")).map_err(map_io)?;
            let payload: Value = serde_json::from_str(&raw).map_err(|err| {
                CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    format!("declared symlink backup is invalid: {err}"),
                )
            })?;
            let target = payload["target"].as_str().ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    "declared symlink backup has no target",
                )
            })?;
            digest_value(&json!({"target": target}))
        }
        _ => Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "declared backup kind is invalid",
        )),
    }
}

pub(super) fn prepare_projection_stages(
    app: &App,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    prepare_projection_stages_from(
        app,
        plan,
        request_id,
        journal_path,
        journal,
        &app.ctx.skill_path(&plan.skill),
    )
}

pub(super) fn rotate_projection_stages(
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let generation = uuid::Uuid::new_v4().hyphenated().to_string();
    for attempt in &mut journal.ownership_attempts {
        if journal
            .projections
            .iter()
            .any(|projection| projection.staging_owner == attempt.destination)
        {
            attempt.state = match attempt.state {
                OwnershipAttemptState::Activated => OwnershipAttemptState::Retained,
                OwnershipAttemptState::Allocated | OwnershipAttemptState::Ready => {
                    OwnershipAttemptState::Abandoned
                }
                state => state,
            };
        }
    }
    for (index, projection) in journal.projections.iter_mut().enumerate() {
        let parent = Path::new(&projection.materialized_path)
            .parent()
            .ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "projection has no parent")
            })?;
        let owner = parent.join(format!(
            ".loom-projection-stage-{}-{index}-{generation}.owner",
            journal.plan_id
        ));
        let proof = new_owner_proof(&journal.plan_id);
        journal
            .ownership_attempts
            .push(allocate_attempt(&owner, &journal.plan_id, &proof)?);
        projection.staging_path = owner.join("stage").display().to_string();
        projection.staging_owner = owner.display().to_string();
        projection.owner_proof = proof;
        projection.activated_fingerprint = None;
    }
    save_journal(journal_path, journal)
}

fn prepare_projection_stages_from(
    app: &App,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    source: &Path,
) -> std::result::Result<(), CommandFailure> {
    if plan.projections.is_empty() {
        return Ok(());
    }
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
    for (index, effect) in plan.projections.iter().enumerate() {
        let materialized_path = Path::new(&journal.projections[index].materialized_path);
        let safe_symlink_noop = effect.effect == "refresh"
            && effect.method == "symlink"
            && projection_path_is_safe_symlink(materialized_path, &app.ctx.skill_path(&plan.skill));
        if safe_symlink_noop {
            journal.projections[index].activated_fingerprint =
                Some(convergence_projection_fingerprint(materialized_path)?);
            continue;
        }
        let owner = journal.projections[index].staging_owner.clone();
        let proof = journal.projections[index].owner_proof.clone();
        fs::create_dir_all(Path::new(&owner).parent().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "projection stage has no target root",
            )
        })?)
        .map_err(map_io)?;
        activate_owned_dir(journal_path, journal, Path::new(&owner), &proof)?;
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
            Path::new(&journal.projections[index].staging_path),
        )?;
        journal.projections[index].activated_fingerprint =
            Some(convergence_projection_fingerprint(Path::new(
                &journal.projections[index].staging_path,
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
