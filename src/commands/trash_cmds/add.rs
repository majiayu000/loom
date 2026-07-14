use super::activation::{
    AppliedTrashActivation, TrashActivationPlan, apply_trash_activation, plan_trash_activation,
    rollback_trash_source_and_activation,
};
use super::*;
use crate::commands::skill_cmds::shared::{push_rollback_error, rollback_fault_active};

pub(super) fn run(
    app: &App,
    args: &TrashAddArgs,
    request_id: &str,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    validate_skill_name(&args.skill).map_err(map_arg)?;
    let _workspace = app.ctx.lock_workspace().map_err(map_lock)?;
    app.ensure_write_repo_ready()?;
    let _lock = app.ctx.lock_skill(&args.skill).map_err(map_lock)?;

    let skill_rel = format!("skills/{}", args.skill);
    let skill_path = app.ctx.root.join(&skill_rel);
    ensure_skill_exists(&app.ctx, &args.skill)?;
    let source_commit = gitops::head(&app.ctx).map_err(map_git)?;

    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let activation_plan = plan_trash_activation(&paths, &skill_path, &args.skill)?;
    let previous_index = gitops::snapshot_index(&app.ctx).map_err(map_git)?;
    let registry_layout_backup = backup_registry_layout(app, &paths).map_err(map_registry_state)?;
    if let Err(err) = paths.ensure_layout() {
        let rollback_errors =
            rollback_prepared_state(app, &paths, &registry_layout_backup, &previous_index);
        return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
    }
    let registry_backup = match snapshot_registry_audit_state(&paths) {
        Ok(backup) => backup,
        Err(err) => {
            let rollback_errors =
                rollback_prepared_state(app, &paths, &registry_layout_backup, &previous_index);
            return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
        }
    };
    let activation_impact = activation_plan.impact_json();
    let trash_id = new_trash_id(&args.skill);
    let entry_path = app.ctx.root.join("trash").join(&trash_id);
    let trash_skill_path = entry_path.join("skill");

    let activation_applied = match apply_trash_activation(&paths, &activation_plan) {
        Ok(applied) => applied,
        Err(err) => {
            let rollback_errors =
                rollback_prepared_state(app, &paths, &registry_layout_backup, &previous_index);
            return Err(append_rollback_errors(err, rollback_errors));
        }
    };
    let rollback = TrashAddRollback {
        app,
        paths: &paths,
        activation_plan: &activation_plan,
        activation_applied: &activation_applied,
        skill_path: &skill_path,
        trash_skill_path: &trash_skill_path,
        entry_path: &entry_path,
        audit_backup: &registry_backup,
        registry_layout_backup: &registry_layout_backup,
        previous_index: &previous_index,
    };
    if let Err(err) = fs::create_dir_all(&entry_path) {
        return Err(map_io(err).with_rollback_errors(rollback.run()));
    }
    if let Err(err) = fs::rename(&skill_path, &trash_skill_path) {
        return Err(map_io(err).with_rollback_errors(rollback.run()));
    }

    let metadata = TrashMetadata {
        schema_version: TRASH_SCHEMA_VERSION,
        trash_id: trash_id.clone(),
        skill: args.skill.clone(),
        original_path: skill_rel.clone(),
        trashed_at: Utc::now(),
        source_commit: source_commit.clone(),
    };
    if let Err(err) = write_trash_metadata(&entry_path, &metadata) {
        return Err(map_io(err).with_rollback_errors(rollback.run()));
    }

    let op_id = match record_registry_operation(
        &paths,
        "skill.trash.add",
        json!({
            "skill": args.skill,
            "trash_id": trash_id,
            "request_id": request_id
        }),
        json!({
            "trash_id": trash_id,
            "trash_path": format!("trash/{}", trash_id),
            "source_commit": source_commit,
            "activation_impact": activation_impact
        }),
    ) {
        Ok(op_id) => op_id,
        Err(err) => {
            return Err(map_registry_state(err).with_rollback_errors(rollback.run()));
        }
    };

    if let Err(err) =
        stage_trash_commit_paths(&app.ctx, &[&skill_rel, &format!("trash/{}", trash_id)])
    {
        return Err(err.with_rollback_errors(rollback.run()));
    }

    let commit = match commit_trash_paths(
        &app.ctx,
        &[&skill_rel, &format!("trash/{}", trash_id)],
        &format!("trash({}): move to trash", args.skill),
    ) {
        Ok(commit) => commit,
        Err(err) => {
            return Err(map_git(err).with_rollback_errors(rollback.run()));
        }
    };

    remove_registry_layout_backups(&registry_layout_backup);

    let mut meta = Meta {
        op_id: Some(op_id),
        ..Meta::default()
    };
    maybe_autosync_or_queue(
        &app.ctx,
        "trash_add",
        request_id,
        json!({"skill": args.skill, "trash_id": trash_id, "commit": commit}),
        &mut meta,
    )?;

    Ok((
        json!({
            "skill": args.skill,
            "trash_id": trash_id,
            "trash_path": format!("trash/{}", trash_id),
            "commit": commit,
            "activation_impact": activation_impact
        }),
        meta,
    ))
}

pub(super) fn plan(
    app: &App,
    args: &TrashAddArgs,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    validate_skill_name(&args.skill).map_err(map_arg)?;
    let skill_rel = format!("skills/{}", args.skill);
    let skill_path = app.ctx.root.join(&skill_rel);
    ensure_skill_exists(&app.ctx, &args.skill)?;
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let activation_impact = plan_trash_activation(&paths, &skill_path, &args.skill)?.impact_json();

    Ok((
        json!({
            "skill": args.skill,
            "dry_run": true,
            "would_move": true,
            "original_path": skill_rel,
            "trash_path": format!("trash/{}", new_trash_id(&args.skill)),
            "activation_impact": activation_impact,
            "would_record_operation": true,
            "would_commit": true
        }),
        Meta::default(),
    ))
}

struct TrashAddRollback<'a> {
    app: &'a App,
    paths: &'a RegistryStatePaths,
    activation_plan: &'a TrashActivationPlan,
    activation_applied: &'a AppliedTrashActivation,
    skill_path: &'a Path,
    trash_skill_path: &'a Path,
    entry_path: &'a Path,
    audit_backup: &'a RegistryAuditStateBackup,
    registry_layout_backup: &'a RegistryLayoutBackup,
    previous_index: &'a gitops::IndexSnapshot,
}

impl TrashAddRollback<'_> {
    fn run(&self) -> Vec<Value> {
        let mut errors = rollback_trash_source_and_activation(
            self.paths,
            self.activation_plan,
            self.activation_applied,
            self.skill_path,
            self.trash_skill_path,
            self.entry_path,
        );
        errors.extend(restore_registry_audit_state_best_effort(
            self.paths,
            self.audit_backup,
        ));
        errors.extend(restore_registry_layout_best_effort(
            self.paths,
            self.registry_layout_backup,
        ));
        errors.extend(restore_index_best_effort(self.app, self.previous_index));
        errors
    }
}

struct RegistryLayoutBackup {
    registry: Option<Value>,
    legacy_v3: Option<Value>,
}

fn backup_registry_layout(
    app: &App,
    paths: &RegistryStatePaths,
) -> anyhow::Result<RegistryLayoutBackup> {
    let registry = backup_path_if_exists(&app.ctx, &paths.registry_dir, "trash-registry-layout")?;
    let legacy_v3 = match backup_path_if_exists(
        &app.ctx,
        &paths.state_dir.join("v3"),
        "trash-legacy-registry-layout",
    ) {
        Ok(backup) => backup,
        Err(err) => {
            remove_temp_backup_best_effort(registry.as_ref());
            return Err(err);
        }
    };
    Ok(RegistryLayoutBackup {
        registry,
        legacy_v3,
    })
}

fn rollback_prepared_state(
    app: &App,
    paths: &RegistryStatePaths,
    registry_layout_backup: &RegistryLayoutBackup,
    previous_index: &gitops::IndexSnapshot,
) -> Vec<Value> {
    let mut errors = restore_registry_layout_best_effort(paths, registry_layout_backup);
    errors.extend(restore_index_best_effort(app, previous_index));
    errors
}

fn restore_registry_layout_best_effort(
    paths: &RegistryStatePaths,
    backup: &RegistryLayoutBackup,
) -> Vec<Value> {
    let mut errors = restore_layout_path_best_effort(
        &paths.registry_dir,
        backup.registry.as_ref(),
        "restore_registry_layout",
        "remove_registry_layout",
    );
    errors.extend(restore_layout_path_best_effort(
        &paths.state_dir.join("v3"),
        backup.legacy_v3.as_ref(),
        "restore_legacy_registry_layout",
        "remove_legacy_registry_layout",
    ));
    errors
}

fn restore_layout_path_best_effort(
    path: &Path,
    backup: Option<&Value>,
    restore_step: &str,
    remove_step: &str,
) -> Vec<Value> {
    let mut errors = Vec::new();
    let step = if backup.is_some() {
        restore_step
    } else {
        remove_step
    };
    if rollback_fault_active(step) {
        push_rollback_error(&mut errors, step, format!("fault injected at {step}"));
        return errors;
    }
    let result = match backup {
        Some(backup) => restore_path_from_backup(path, backup),
        None => remove_path_if_exists(path).map_err(anyhow::Error::from),
    };
    if let Err(err) = result {
        push_rollback_error(&mut errors, step, err);
    } else {
        remove_temp_backup_best_effort(backup);
    }
    errors
}

fn restore_index_best_effort(app: &App, previous_index: &gitops::IndexSnapshot) -> Vec<Value> {
    let mut errors = Vec::new();
    if rollback_fault_active("restore_git_index") {
        push_rollback_error(
            &mut errors,
            "restore_git_index",
            "fault injected at restore_git_index",
        );
    } else if let Err(err) = gitops::restore_index(&app.ctx, previous_index) {
        push_rollback_error(&mut errors, "restore_git_index", err);
    }
    errors
}

fn remove_registry_layout_backups(backup: &RegistryLayoutBackup) {
    remove_temp_backup_best_effort(backup.registry.as_ref());
    remove_temp_backup_best_effort(backup.legacy_v3.as_ref());
}

fn append_rollback_errors(mut failure: CommandFailure, mut errors: Vec<Value>) -> CommandFailure {
    if errors.is_empty() {
        return failure;
    }
    if let Some(existing) = failure
        .details
        .get_mut("rollback_errors")
        .and_then(Value::as_array_mut)
    {
        existing.append(&mut errors);
        failure
    } else {
        failure.with_rollback_errors(errors)
    }
}
