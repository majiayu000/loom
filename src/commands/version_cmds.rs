use std::fs;
use std::path::Path;

use serde_json::json;

use crate::cli::{DiffArgs, ReleaseArgs, RollbackArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::helpers::{
    backup_path_if_exists, commit_registry_state, ensure_skill_exists, map_arg, map_git, map_lock,
    map_registry_state, maybe_autosync_or_queue, record_registry_observation,
    record_registry_operation, restore_path_from_backup, validate_skill_name,
};
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
            backup_path_if_exists(&self.ctx, &paths.registry_dir, "registry-layout")
                .map_err(map_registry_state)?;
        paths.ensure_layout().map_err(map_registry_state)?;

        gitops::create_annotated_tag(
            &self.ctx,
            &tag,
            &format!("release {} {}", args.skill, args.version),
        )
        .map_err(map_git)?;

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
                remove_backup_path_best_effort(registry_layout_backup.as_ref());
                result
            }
            Err(err) => {
                reset_command_created_commit_best_effort(self, &previous_head);
                restore_path_best_effort(&paths.registry_dir, registry_layout_backup.as_ref());
                remove_backup_path_best_effort(registry_layout_backup.as_ref());
                let _ = gitops::restore_index(&self.ctx, &previous_index);
                delete_tag_best_effort(self, &tag);
                return Err(err);
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
        gitops::checkout_path_from_ref(&self.ctx, &reference, Path::new(&skill_rel))
            .map_err(map_git)?;
        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;

        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        if !changed {
            return Ok((
                json!({"skill": args.skill, "reference": reference, "noop": true}),
                Meta::default(),
            ));
        }

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let registry_layout_backup =
            backup_path_if_exists(&self.ctx, &paths.registry_dir, "registry-layout")
                .map_err(map_registry_state)?;
        paths.ensure_layout().map_err(map_registry_state)?;

        let message = format!("rollback({}): restore from {}", args.skill, reference);
        let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;

        let post_audit: std::result::Result<(Option<String>, Meta), CommandFailure> = (|| {
            let op_id = record_registry_operation(
                &paths,
                "skill.rollback",
                json!({
                    "skill": args.skill,
                    "reference": reference,
                    "request_id": request_id
                }),
                json!({
                    "commit": commit,
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
                    "state_commit": state_commit
                }),
                &mut meta,
            )?;
            Ok((state_commit, meta))
        })();
        let (state_commit, mut meta) = match post_audit {
            Ok(result) => {
                remove_backup_path_best_effort(skill_backup.as_ref());
                remove_backup_path_best_effort(registry_layout_backup.as_ref());
                result
            }
            Err(err) => {
                reset_command_created_commit_best_effort(self, &previous_head);
                restore_path_best_effort(&skill_path, skill_backup.as_ref());
                remove_backup_path_best_effort(skill_backup.as_ref());
                restore_path_best_effort(&paths.registry_dir, registry_layout_backup.as_ref());
                remove_backup_path_best_effort(registry_layout_backup.as_ref());
                let _ = gitops::restore_index(&self.ctx, &previous_index);
                return Err(err);
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

fn reset_command_created_commit_best_effort(app: &App, previous_head: &str) {
    let _ = gitops::run_git_allow_failure(&app.ctx, &["reset", "--soft", previous_head]);
}

fn restore_path_best_effort(path: &Path, backup: Option<&serde_json::Value>) {
    if let Some(backup) = backup {
        let _ = restore_path_from_backup(path, backup);
    } else {
        let _ = crate::state::remove_path_if_exists(path);
    }
}

fn remove_backup_path_best_effort(backup: Option<&serde_json::Value>) {
    let Some(path) = backup
        .and_then(|backup| backup.get("backup_path"))
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
    else {
        return;
    };
    let _ = crate::state::remove_path_if_exists(path);
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
