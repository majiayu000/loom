use super::shared::*;
use super::*;

impl App {
    pub fn cmd_save(
        &self,
        args: &SaveArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;
        let skill_rel = format!("skills/{}", args.skill);
        let skill_path = self.ctx.root.join(&skill_rel);
        if !skill_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }
        if args.preflight {
            self.ensure_save_preflight(&args.skill)?;
        }

        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;
        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        if !changed {
            return Ok((json!({"skill": args.skill, "noop": true}), Meta::default()));
        }

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let had_registry_layout = paths.registry_dir.exists();
        let had_legacy_layout = paths.legacy_state_dir_exists();
        let legacy_layout_backup = if had_legacy_layout && !had_registry_layout {
            backup_path_if_exists(
                &self.ctx,
                &paths.state_dir.join("v3"),
                "legacy-registry-layout",
            )
            .map_err(map_registry_state)?
        } else {
            None
        };
        if let Err(err) = paths.ensure_layout() {
            let mut rollback_errors = rollback_registry_layout_after_failure(
                &self.ctx,
                &paths,
                had_registry_layout,
                had_legacy_layout,
                legacy_layout_backup.as_ref(),
            );
            if let Err(reset_err) = gitops::run_git(&self.ctx, &["reset", "HEAD", "--", &skill_rel])
            {
                push_rollback_error(&mut rollback_errors, "reset_staged_skill", reset_err);
            }
            return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
        }
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let message = args
            .message
            .clone()
            .unwrap_or_else(|| format!("save({}): event", args.skill));
        let op_id = match record_registry_operation(
            &paths,
            "skill.save",
            json!({
                "skill": args.skill,
                "request_id": request_id
            }),
            json!({
                "noop": false
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                let rollback_errors = rollback_save_after_failure(
                    &self.ctx,
                    &paths,
                    &registry_backup,
                    had_registry_layout,
                    had_legacy_layout,
                    legacy_layout_backup.as_ref(),
                    &skill_rel,
                );
                return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
            }
        };
        if let Err(err) = maybe_skill_fault("skill_save_after_operation") {
            let rollback_errors = rollback_save_after_failure(
                &self.ctx,
                &paths,
                &registry_backup,
                had_registry_layout,
                had_legacy_layout,
                legacy_layout_backup.as_ref(),
                &skill_rel,
            );
            return Err(err.with_rollback_errors(rollback_errors));
        }
        if let Err(err) = stage_registry_state(&self.ctx, &paths) {
            let rollback_errors = rollback_save_after_failure(
                &self.ctx,
                &paths,
                &registry_backup,
                had_registry_layout,
                had_legacy_layout,
                legacy_layout_backup.as_ref(),
                &skill_rel,
            );
            return Err(err.with_rollback_errors(rollback_errors));
        }
        let commit = match gitops::commit(&self.ctx, &message) {
            Ok(commit) => commit,
            Err(err) => {
                let rollback_errors = rollback_save_after_failure(
                    &self.ctx,
                    &paths,
                    &registry_backup,
                    had_registry_layout,
                    had_legacy_layout,
                    legacy_layout_backup.as_ref(),
                    &skill_rel,
                );
                return Err(map_git(err).with_rollback_errors(rollback_errors));
            }
        };
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        remove_backup_path_best_effort(legacy_layout_backup.as_ref());
        maybe_autosync_or_queue(
            &self.ctx,
            "save",
            request_id,
            json!({"skill": args.skill, "commit": commit}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "commit": commit, "noop": false}),
            meta,
        ))
    }
}

fn rollback_save_after_failure(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
    had_registry_layout: bool,
    had_legacy_layout: bool,
    legacy_layout_backup: Option<&serde_json::Value>,
    skill_rel: &str,
) -> Vec<Value> {
    let mut errors = Vec::new();
    match gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", skill_rel]) {
        Ok(output) if output.status.success() => {}
        Ok(output) => push_rollback_error(
            &mut errors,
            "reset_staged_skill",
            String::from_utf8_lossy(&output.stderr).trim(),
        ),
        Err(err) => push_rollback_error(&mut errors, "reset_staged_skill", err),
    }
    errors.extend(rollback_registry_audit_after_failure(
        ctx,
        paths,
        registry_backup,
        had_registry_layout,
        had_legacy_layout,
        legacy_layout_backup,
    ));
    errors
}
