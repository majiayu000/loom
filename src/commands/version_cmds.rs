use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::{DiffArgs, ReleaseArgs, RollbackArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::file_ops::{backup_path_if_exists, restore_path_from_backup};
use super::helpers::{
    commit_registry_state, ensure_skill_exists, map_arg, map_git, map_lock, map_registry_state,
    validate_skill_name,
};
use super::projections::{
    maybe_autosync_or_queue, record_registry_observation, record_registry_operation,
};
use super::skill_safety::security_diff_report;
use super::{App, CommandFailure};

impl App {
    pub fn cmd_release(
        &self,
        args: &ReleaseArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let previous_index = gitops::snapshot_index(&self.ctx).map_err(map_git)?;
        let tag = format!("release/{}/{}", args.skill, args.version);
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let registry_layout_backup =
            backup_registry_layout(&self.ctx, &paths).map_err(map_registry_state)?;
        if let Err(err) = paths.ensure_layout() {
            restore_registry_layout_best_effort(&paths, &registry_layout_backup);
            remove_registry_layout_backups_best_effort(&registry_layout_backup);
            let _ = gitops::restore_index(&self.ctx, &previous_index);
            return Err(map_registry_state(err));
        }

        if let Err(err) = gitops::create_annotated_tag(
            &self.ctx,
            &tag,
            &format!("release {} {}", args.skill, args.version),
        ) {
            restore_registry_layout_best_effort(&paths, &registry_layout_backup);
            remove_registry_layout_backups_best_effort(&registry_layout_backup);
            let _ = gitops::restore_index(&self.ctx, &previous_index);
            return Err(map_git(err));
        }

        let post_audit: std::result::Result<(Option<String>, Meta), CommandFailure> = (|| {
            let op_id = record_registry_operation(
                &paths,
                "skill.release",
                json!({
                    "skill": args.skill,
                    "version": args.version,
                    "tag": tag,
                    "request_id": request_id
                }),
                json!({
                    "tag": tag
                }),
            )
            .map_err(map_registry_state)?;
            record_skill_projection_observations(
                &paths,
                &args.skill,
                "released",
                None,
                None,
                Some(tag.clone()),
            )
            .map_err(map_registry_state)?;
            let state_commit = commit_registry_state(
                &self.ctx,
                &format!("release({}): record registry operation", args.skill),
            )?;
            maybe_version_fault("skill_release_after_state_commit")?;
            let mut meta = Meta {
                op_id: Some(op_id),
                ..Meta::default()
            };
            maybe_autosync_or_queue(
                &self.ctx,
                "release",
                request_id,
                json!({"skill": args.skill, "tag": tag, "state_commit": state_commit}),
                &mut meta,
            )?;
            Ok((state_commit, meta))
        })();
        let (state_commit, meta) = match post_audit {
            Ok(result) => {
                remove_registry_layout_backups_best_effort(&registry_layout_backup);
                result
            }
            Err(err) => {
                let mut rollback_errors =
                    reset_command_created_commit_best_effort(self, &previous_head);
                rollback_errors.extend(restore_registry_layout_best_effort(
                    &paths,
                    &registry_layout_backup,
                ));
                remove_registry_layout_backups_best_effort(&registry_layout_backup);
                rollback_errors.extend(restore_index_best_effort(&self.ctx, &previous_index));
                delete_tag_best_effort(self, &tag);
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };

        Ok((
            json!({"skill": args.skill, "version": args.version, "tag": tag, "state_commit": state_commit}),
            meta,
        ))
    }

    pub fn cmd_rollback(
        &self,
        args: &RollbackArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        if args.to.is_some() && args.steps.is_some() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--to and --steps are mutually exclusive",
            ));
        }

        let reference = match (&args.to, args.steps) {
            (Some(r), _) => r.clone(),
            (None, Some(n)) => format!("HEAD~{}", n),
            (None, None) => "HEAD~1".to_string(),
        };

        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;
        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let previous_index = gitops::snapshot_index(&self.ctx).map_err(map_git)?;
        gitops::resolve_ref(&self.ctx, &reference).map_err(map_git)?;

        let skill_rel = format!("skills/{}", args.skill);
        let skill_path = self.ctx.root.join(&skill_rel);
        let skill_backup = backup_path_if_exists(&self.ctx, &skill_path, "skill-rollback")
            .map_err(map_registry_state)?;
        if let Err(err) =
            gitops::checkout_path_from_ref(&self.ctx, &reference, Path::new(&skill_rel))
        {
            restore_path_best_effort(
                &skill_path,
                skill_backup.as_ref(),
                "restore_skill_path",
                "remove_skill_path",
            );
            remove_backup_path_best_effort(skill_backup.as_ref());
            let _ = gitops::restore_index(&self.ctx, &previous_index);
            return Err(map_git(err));
        }
        if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
            let mut rollback_errors = restore_path_best_effort(
                &skill_path,
                skill_backup.as_ref(),
                "restore_skill_path",
                "remove_skill_path",
            );
            remove_backup_path_best_effort(skill_backup.as_ref());
            rollback_errors.extend(restore_index_best_effort(&self.ctx, &previous_index));
            return Err(map_git(err).with_rollback_errors(rollback_errors));
        }

        let changed = match gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel)) {
            Ok(changed) => changed,
            Err(err) => {
                restore_path_best_effort(
                    &skill_path,
                    skill_backup.as_ref(),
                    "restore_skill_path",
                    "remove_skill_path",
                );
                remove_backup_path_best_effort(skill_backup.as_ref());
                let _ = gitops::restore_index(&self.ctx, &previous_index);
                return Err(map_git(err));
            }
        };
        if !changed {
            remove_backup_path_best_effort(skill_backup.as_ref());
            let _ = gitops::restore_index(&self.ctx, &previous_index);
            return Ok((
                json!({"skill": args.skill, "reference": reference, "noop": true}),
                Meta::default(),
            ));
        }

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let registry_layout_backup =
            backup_registry_layout(&self.ctx, &paths).map_err(map_registry_state)?;
        if let Err(err) = paths.ensure_layout() {
            restore_path_best_effort(
                &skill_path,
                skill_backup.as_ref(),
                "restore_skill_path",
                "remove_skill_path",
            );
            remove_backup_path_best_effort(skill_backup.as_ref());
            restore_registry_layout_best_effort(&paths, &registry_layout_backup);
            remove_registry_layout_backups_best_effort(&registry_layout_backup);
            let _ = gitops::restore_index(&self.ctx, &previous_index);
            return Err(map_registry_state(err));
        }

        let previous_short = previous_head.chars().take(12).collect::<String>();
        let ts = Utc::now().format("%Y%m%dT%H%M%S%fZ");
        let recovery_ref = format!("recovery/{}/{}-{}", args.skill, ts, previous_short);
        if let Err(err) = gitops::create_annotated_tag(
            &self.ctx,
            &recovery_ref,
            &format!("recovery before rollback {}", args.skill),
        ) {
            restore_path_best_effort(
                &skill_path,
                skill_backup.as_ref(),
                "restore_skill_path",
                "remove_skill_path",
            );
            remove_backup_path_best_effort(skill_backup.as_ref());
            restore_registry_layout_best_effort(&paths, &registry_layout_backup);
            remove_registry_layout_backups_best_effort(&registry_layout_backup);
            let _ = gitops::restore_index(&self.ctx, &previous_index);
            return Err(map_git(err));
        }

        let message = format!("rollback({}): restore from {}", args.skill, reference);
        let commit = match gitops::commit(&self.ctx, &message) {
            Ok(commit) => commit,
            Err(err) => {
                delete_tag_best_effort(self, &recovery_ref);
                restore_path_best_effort(
                    &skill_path,
                    skill_backup.as_ref(),
                    "restore_skill_path",
                    "remove_skill_path",
                );
                remove_backup_path_best_effort(skill_backup.as_ref());
                restore_registry_layout_best_effort(&paths, &registry_layout_backup);
                remove_registry_layout_backups_best_effort(&registry_layout_backup);
                let _ = gitops::restore_index(&self.ctx, &previous_index);
                return Err(map_git(err));
            }
        };

        let post_audit: std::result::Result<(Option<String>, Meta), CommandFailure> = (|| {
            let op_id = record_registry_operation(
                &paths,
                "skill.rollback",
                json!({
                    "skill": args.skill,
                    "reference": reference,
                    "recovery_ref": recovery_ref,
                    "request_id": request_id
                }),
                json!({
                    "commit": commit,
                    "recovery_ref": recovery_ref,
                    "noop": false
                }),
            )
            .map_err(map_registry_state)?;
            record_skill_projection_observations(
                &paths,
                &args.skill,
                "rollback",
                Some(skill_rel.clone()),
                Some(previous_head.clone()),
                Some(reference.clone()),
            )
            .map_err(map_registry_state)?;
            let state_commit = commit_registry_state(
                &self.ctx,
                &format!("rollback({}): record registry operation", args.skill),
            )?;
            maybe_version_fault("skill_rollback_after_state_commit")?;
            let mut meta = Meta {
                op_id: Some(op_id),
                ..Meta::default()
            };
            maybe_autosync_or_queue(
                &self.ctx,
                "rollback",
                request_id,
                json!({
                    "skill": args.skill,
                    "commit": commit,
                    "reference": reference,
                    "recovery_ref": recovery_ref,
                    "state_commit": state_commit
                }),
                &mut meta,
            )?;
            Ok((state_commit, meta))
        })();
        let (state_commit, mut meta) = match post_audit {
            Ok(result) => {
                remove_backup_path_best_effort(skill_backup.as_ref());
                remove_registry_layout_backups_best_effort(&registry_layout_backup);
                result
            }
            Err(err) => {
                let mut rollback_errors = Vec::new();
                delete_tag_best_effort(self, &recovery_ref);
                rollback_errors.extend(reset_command_created_commit_best_effort(
                    self,
                    &previous_head,
                ));
                rollback_errors.extend(restore_path_best_effort(
                    &skill_path,
                    skill_backup.as_ref(),
                    "restore_skill_path",
                    "remove_skill_path",
                ));
                remove_backup_path_best_effort(skill_backup.as_ref());
                rollback_errors.extend(restore_registry_layout_best_effort(
                    &paths,
                    &registry_layout_backup,
                ));
                remove_registry_layout_backups_best_effort(&registry_layout_backup);
                rollback_errors.extend(restore_index_best_effort(&self.ctx, &previous_index));
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };

        if let Ok(Some(snapshot)) = paths.maybe_load_snapshot() {
            let stale: Vec<_> = snapshot
                .projections
                .projections
                .iter()
                .filter(|p| p.skill_id == args.skill && p.method != "symlink")
                .map(|p| p.instance_id.clone())
                .collect();
            if !stale.is_empty() {
                meta.warnings.push(format!(
                    "rollback does not update live projections; re-run 'loom skill project' for: {}",
                    stale.join(", ")
                ));
            }
        }

        Ok((
            json!({
                "skill": args.skill,
                "reference": reference,
                "recovery_ref": recovery_ref,
                "commit": commit,
                "state_commit": state_commit,
                "noop": false
            }),
            meta,
        ))
    }

    pub fn cmd_diff(
        &self,
        args: &DiffArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        if args.security {
            return Ok((
                security_diff_report(&self.ctx, &args.skill, &args.from, &args.to)?,
                Meta::default(),
            ));
        }
        let skill_rel = format!("skills/{}", args.skill);
        let diff = gitops::diff_path(&self.ctx, &args.from, &args.to, Path::new(&skill_rel))
            .map_err(map_git)?;
        Ok((
            json!({"skill": args.skill, "from": args.from, "to": args.to, "diff": diff}),
            Meta::default(),
        ))
    }
}

fn delete_tag_best_effort(app: &App, tag: &str) {
    let _ = gitops::run_git_allow_failure(&app.ctx, &["tag", "-d", tag]);
}

fn reset_command_created_commit_best_effort(app: &App, previous_head: &str) -> Vec<Value> {
    let mut errors = Vec::new();
    if maybe_push_rollback_fault(&mut errors, "reset_command_created_commit") {
        return errors;
    }
    match gitops::run_git_allow_failure(&app.ctx, &["reset", "--soft", previous_head]) {
        Ok(output) if output.status.success() => {}
        Ok(output) => push_rollback_error(
            &mut errors,
            "reset_command_created_commit",
            String::from_utf8_lossy(&output.stderr).trim(),
        ),
        Err(err) => push_rollback_error(&mut errors, "reset_command_created_commit", err),
    }
    errors
}

fn restore_path_best_effort(
    path: &Path,
    backup: Option<&serde_json::Value>,
    restore_step: &str,
    remove_step: &str,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Some(backup) = backup {
        if !maybe_push_rollback_fault(&mut errors, restore_step)
            && let Err(err) = restore_path_from_backup(path, backup)
        {
            push_rollback_error(&mut errors, restore_step, err);
        }
    } else {
        if !maybe_push_rollback_fault(&mut errors, remove_step)
            && let Err(err) = crate::fs_util::remove_path_if_exists(path)
        {
            push_rollback_error(&mut errors, remove_step, err);
        }
    }
    errors
}

struct RegistryLayoutBackup {
    registry: Option<serde_json::Value>,
    legacy_v3: Option<serde_json::Value>,
}

fn backup_registry_layout(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
) -> anyhow::Result<RegistryLayoutBackup> {
    Ok(RegistryLayoutBackup {
        registry: backup_path_if_exists(ctx, &paths.registry_dir, "registry-layout")?,
        legacy_v3: backup_path_if_exists(
            ctx,
            &paths.state_dir.join("v3"),
            "legacy-registry-layout",
        )?,
    })
}

fn restore_registry_layout_best_effort(
    paths: &RegistryStatePaths,
    backup: &RegistryLayoutBackup,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if backup.registry.is_some() {
        if maybe_push_rollback_fault(&mut errors, "restore_registry_layout") {
            // Fault injection intentionally skips the actual restore.
        } else {
            errors.extend(restore_path_best_effort(
                &paths.registry_dir,
                backup.registry.as_ref(),
                "restore_registry_layout",
                "remove_registry_layout",
            ));
        }
    } else {
        if !maybe_push_rollback_fault(&mut errors, "remove_registry_layout")
            && let Err(err) = crate::fs_util::remove_path_if_exists(&paths.registry_dir)
        {
            push_rollback_error(&mut errors, "remove_registry_layout", err);
        }
    }

    if backup.legacy_v3.is_some() {
        if maybe_push_rollback_fault(&mut errors, "restore_legacy_registry_layout") {
            // Fault injection intentionally skips the actual restore.
        } else {
            errors.extend(restore_path_best_effort(
                &paths.state_dir.join("v3"),
                backup.legacy_v3.as_ref(),
                "restore_legacy_registry_layout",
                "remove_legacy_registry_layout",
            ));
        }
    }
    errors
}

fn restore_index_best_effort(
    ctx: &crate::state::AppContext,
    previous_index: &gitops::IndexSnapshot,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if !maybe_push_rollback_fault(&mut errors, "restore_git_index")
        && let Err(err) = gitops::restore_index(ctx, previous_index)
    {
        push_rollback_error(&mut errors, "restore_git_index", err);
    }
    errors
}

fn rollback_fault_active(tag: &str) -> bool {
    std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref() == Some(tag)
}

fn push_rollback_error(errors: &mut Vec<Value>, step: &str, message: impl ToString) {
    errors.push(json!({
        "step": step,
        "message": message.to_string(),
    }));
}

fn maybe_push_rollback_fault(errors: &mut Vec<Value>, step: &str) -> bool {
    if rollback_fault_active(step) {
        push_rollback_error(errors, step, format!("fault injected at {}", step));
        return true;
    }
    false
}

fn remove_registry_layout_backups_best_effort(backup: &RegistryLayoutBackup) {
    remove_backup_path_best_effort(backup.registry.as_ref());
    remove_backup_path_best_effort(backup.legacy_v3.as_ref());
}

fn remove_backup_path_best_effort(backup: Option<&serde_json::Value>) {
    let Some(path) = backup
        .and_then(|backup| backup.get("backup_path"))
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
    else {
        return;
    };
    let _ = crate::fs_util::remove_path_if_exists(path);
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
        if let Some(grandparent) = parent.parent() {
            let _ = fs::remove_dir(grandparent);
        }
    }
}

fn maybe_version_fault(tag: &str) -> std::result::Result<(), CommandFailure> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            format!("fault injected at {}", tag),
        ));
    }
    Ok(())
}

fn record_skill_projection_observations(
    paths: &RegistryStatePaths,
    skill_id: &str,
    kind: &str,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> anyhow::Result<()> {
    if let Some(snapshot) = paths.maybe_load_snapshot()? {
        for projection in snapshot
            .projections
            .projections
            .iter()
            .filter(|projection| projection.skill_id == skill_id)
        {
            record_registry_observation(
                paths,
                &projection.instance_id,
                kind,
                path.clone(),
                from.clone(),
                to.clone(),
            )?;
        }
    }
    Ok(())
}
