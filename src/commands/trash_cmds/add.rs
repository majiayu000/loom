use super::activation::{
    AppliedTrashActivation, TrashActivationPlan, apply_trash_activation, plan_trash_activation,
    rollback_trash_activation, rollback_trash_source_and_activation,
};
use super::*;

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

    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    paths.ensure_layout().map_err(map_registry_state)?;
    let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
    let activation_plan = plan_trash_activation(&paths, &skill_path, &args.skill)?;
    let activation_impact = activation_plan.impact_json();
    let source_commit = gitops::head(&app.ctx).map_err(map_git)?;
    let trash_id = new_trash_id(&args.skill);
    let entry_path = app.ctx.root.join("trash").join(&trash_id);
    let trash_skill_path = entry_path.join("skill");

    let activation_applied = apply_trash_activation(&paths, &activation_plan)?;
    if let Err(err) = fs::create_dir_all(&entry_path) {
        let rollback_errors =
            rollback_trash_activation(&paths, &activation_plan, &activation_applied);
        return Err(map_io(err).with_rollback_errors(rollback_errors));
    }
    if let Err(err) = fs::rename(&skill_path, &trash_skill_path) {
        return Err(map_io(err).with_rollback_errors(rollback_all(
            &paths,
            &activation_plan,
            &activation_applied,
            &skill_path,
            &trash_skill_path,
            &entry_path,
            None,
        )));
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
        return Err(map_io(err).with_rollback_errors(rollback_all(
            &paths,
            &activation_plan,
            &activation_applied,
            &skill_path,
            &trash_skill_path,
            &entry_path,
            None,
        )));
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
            let rollback_errors = rollback_all(
                &paths,
                &activation_plan,
                &activation_applied,
                &skill_path,
                &trash_skill_path,
                &entry_path,
                Some(&registry_backup),
            );
            unstage_trash_paths(&app.ctx, &[&skill_rel, &format!("trash/{}", trash_id)]);
            return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
        }
    };

    if let Err(err) =
        stage_trash_commit_paths(&app.ctx, &[&skill_rel, &format!("trash/{}", trash_id)])
    {
        let rollback_errors = rollback_all(
            &paths,
            &activation_plan,
            &activation_applied,
            &skill_path,
            &trash_skill_path,
            &entry_path,
            Some(&registry_backup),
        );
        unstage_trash_paths(&app.ctx, &[&skill_rel, &format!("trash/{}", trash_id)]);
        return Err(err.with_rollback_errors(rollback_errors));
    }

    let commit = match commit_trash_paths(
        &app.ctx,
        &[&skill_rel, &format!("trash/{}", trash_id)],
        &format!("trash({}): move to trash", args.skill),
    ) {
        Ok(commit) => commit,
        Err(err) => {
            let rollback_errors = rollback_all(
                &paths,
                &activation_plan,
                &activation_applied,
                &skill_path,
                &trash_skill_path,
                &entry_path,
                Some(&registry_backup),
            );
            unstage_trash_paths(&app.ctx, &[&skill_rel, &format!("trash/{}", trash_id)]);
            return Err(map_git(err).with_rollback_errors(rollback_errors));
        }
    };

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

fn rollback_all(
    paths: &RegistryStatePaths,
    activation_plan: &TrashActivationPlan,
    activation_applied: &AppliedTrashActivation,
    skill_path: &Path,
    trash_skill_path: &Path,
    entry_path: &Path,
    audit_backup: Option<&RegistryAuditStateBackup>,
) -> Vec<Value> {
    let mut errors = rollback_trash_source_and_activation(
        paths,
        activation_plan,
        activation_applied,
        skill_path,
        trash_skill_path,
        entry_path,
    );
    if let Some(audit_backup) = audit_backup {
        errors.extend(restore_registry_audit_state_best_effort(
            paths,
            audit_backup,
        ));
    }
    errors
}
