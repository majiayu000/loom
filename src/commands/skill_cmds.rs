use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{
    AddArgs, CaptureArgs, DiffArgs, ProjectArgs, ReleaseArgs, RollbackArgs, SaveArgs,
    SkillOnlyArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;
use crate::state_model::{V3BindingRule, V3ProjectionInstance};
use crate::types::ErrorCode;

use super::helpers::{
    backup_path_if_exists, copy_dir_recursive, copy_dir_recursive_without_symlinks,
    ensure_skill_exists, map_arg, map_git, map_io, map_lock, map_project_io, map_v3_state,
    maybe_autosync_or_queue, projection_instance_id, projection_method_as_str,
    project_skill_to_target, record_v3_operation, resolve_capture_projection, rollback_added_skill,
    update_projection_after_capture, upsert_projection, upsert_rule, validate_projection_method,
    validate_skill_name,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_add(
        &self,
        args: &AddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.name).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let dst = self.ctx.skill_path(&args.name);
        if dst.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("skill '{}' already exists", args.name),
            ));
        }

        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-add-{}", Uuid::new_v4()));
        let staging_skill = staging_root.join(&args.name);
        let clone_tmp = staging_root.join("clone");

        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        if Path::new(&args.source).exists() {
            if let Err(err) =
                copy_dir_recursive_without_symlinks(Path::new(&args.source), &staging_skill)
            {
                cleanup_staging();
                return Err(map_io(err));
            }
        } else {
            let source = args.source.as_str();
            let clone = gitops::run_git_allow_failure(
                &self.ctx,
                &[
                    "clone",
                    "--depth",
                    "1",
                    source,
                    clone_tmp.to_string_lossy().as_ref(),
                ],
            )
            .map_err(map_git)?;
            if !clone.status.success() {
                let stderr = String::from_utf8_lossy(&clone.stderr).to_string();
                cleanup_staging();
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("failed to clone source: {}", stderr.trim()),
                ));
            }
            if let Err(err) = copy_dir_recursive_without_symlinks(&clone_tmp, &staging_skill) {
                cleanup_staging();
                return Err(map_io(err));
            }
        }

        if let Err(err) = fs::rename(&staging_skill, &dst) {
            cleanup_staging();
            return Err(map_io(err));
        }
        cleanup_staging();

        let mut meta = Meta::default();
        let skill_rel = format!("skills/{}", args.name);
        if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
            rollback_added_skill(&self.ctx, &skill_rel, &dst);
            return Err(map_git(err));
        }
        let staged = match gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel)) {
            Ok(staged) => staged,
            Err(err) => {
                rollback_added_skill(&self.ctx, &skill_rel, &dst);
                return Err(map_git(err));
            }
        };
        if staged {
            let message = format!("add({}): import {}", args.name, args.source);
            let commit = match gitops::commit(&self.ctx, &message) {
                Ok(commit) => commit,
                Err(err) => {
                    rollback_added_skill(&self.ctx, &skill_rel, &dst);
                    return Err(map_git(err));
                }
            };
            if let Err(err) = maybe_autosync_or_queue(
                &self.ctx,
                "add",
                request_id,
                json!({"skill": args.name, "commit": commit}),
                &mut meta,
            ) {
                rollback_added_skill(&self.ctx, &skill_rel, &dst);
                return Err(err);
            }
        }

        Ok((json!({"skill": args.name, "path": dst}), meta))
    }

    pub fn cmd_project(
        &self,
        args: &ProjectArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;

        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let binding = snapshot.binding(&args.binding).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{}' not found", args.binding),
            )
        })?;

        let target_id = args
            .target
            .clone()
            .unwrap_or_else(|| binding.default_target_id.clone());
        let target = snapshot.target(&target_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", target_id),
            )
        })?;

        if target.ownership != "managed" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' has ownership '{}' and cannot be projected into",
                    target.target_id, target.ownership
                ),
            ));
        }

        validate_projection_method(&target, args.method)?;

        let skill_src = self.ctx.skill_path(&args.skill);
        let target_base = PathBuf::from(&target.path);
        fs::create_dir_all(&target_base).map_err(map_io)?;
        let materialized_path = target_base.join(&args.skill);
        let replaced_projection_backup =
            backup_path_if_exists(&self.ctx, &materialized_path, "project.replace_projection")
                .map_err(map_io)?;
        remove_path_if_exists(&materialized_path).map_err(map_io)?;
        project_skill_to_target(&skill_src, &materialized_path, args.method)
            .map_err(map_project_io(args.method))?;

        let mut rules = snapshot.rules;
        upsert_rule(
            &mut rules,
            V3BindingRule {
                binding_id: binding.binding_id.clone(),
                skill_id: args.skill.clone(),
                target_id: target.target_id.clone(),
                method: projection_method_as_str(args.method).to_string(),
                watch_policy: "observe_only".to_string(),
                created_at: Some(Utc::now()),
            },
        );
        paths.save_rules(&rules).map_err(map_v3_state)?;

        let mut projections = snapshot.projections;
        let instance_id =
            projection_instance_id(&args.skill, &binding.binding_id, &target.target_id);
        let projection = V3ProjectionInstance {
            instance_id: instance_id.clone(),
            skill_id: args.skill.clone(),
            binding_id: binding.binding_id.clone(),
            target_id: target.target_id.clone(),
            materialized_path: materialized_path.display().to_string(),
            method: projection_method_as_str(args.method).to_string(),
            last_applied_rev: gitops::head(&self.ctx).map_err(map_git)?,
            health: "healthy".to_string(),
            observed_drift: Some(false),
            updated_at: Some(Utc::now()),
        };
        upsert_projection(&mut projections, projection.clone());
        paths.save_projections(&projections).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "skill.project",
            json!({
                "skill_id": args.skill,
                "binding_id": binding.binding_id,
                "target_id": target.target_id,
                "method": projection_method_as_str(args.method),
                "request_id": request_id
            }),
            json!({
                "instance_id": instance_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({"projection": projection, "backup": replaced_projection_backup, "noop": false}),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_capture(
        &self,
        args: &CaptureArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let projection = resolve_capture_projection(&snapshot, args)?;
        ensure_skill_exists(&self.ctx, &projection.skill_id)?;

        let skill_rel = format!("skills/{}", projection.skill_id);
        let skill_path = self.ctx.root.join(&skill_rel);
        let live_path = PathBuf::from(&projection.materialized_path);
        if !live_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("projection path '{}' does not exist", live_path.display()),
            ));
        }

        let mut source_backup = None;
        if projection.method != "symlink" {
            let tmp_path = self
                .ctx
                .state_dir
                .join(format!("tmp-capture-{}", Uuid::new_v4()));
            let _ = remove_path_if_exists(&tmp_path);
            copy_dir_recursive(&live_path, &tmp_path).map_err(map_io)?;
            source_backup = backup_path_if_exists(&self.ctx, &skill_path, "capture.replace_source")
                .map_err(map_io)?;
            remove_path_if_exists(&skill_path).map_err(map_io)?;
            fs::rename(&tmp_path, &skill_path).map_err(map_io)?;
        }

        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;
        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        let commit = if changed {
            let message = args.message.clone().unwrap_or_else(|| {
                format!(
                    "capture({}): from {}",
                    projection.skill_id, projection.instance_id
                )
            });
            Some(gitops::commit(&self.ctx, &message).map_err(map_git)?)
        } else {
            None
        };
        let current_rev = gitops::head(&self.ctx).map_err(map_git)?;

        let mut projections = snapshot.projections;
        update_projection_after_capture(&mut projections, &projection.instance_id, &current_rev)?;
        paths.save_projections(&projections).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "skill.capture",
            json!({
                "skill_id": projection.skill_id,
                "binding_id": projection.binding_id,
                "instance_id": projection.instance_id,
                "request_id": request_id
            }),
            json!({
                "instance_id": projection.instance_id,
                "commit": commit
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({
                "capture": {
                    "skill_id": projection.skill_id,
                    "binding_id": projection.binding_id,
                    "instance_id": projection.instance_id,
                    "commit": commit,
                    "backup": source_backup,
                    "noop": !changed
                }
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

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

        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;
        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        if !changed {
            return Ok((json!({"skill": args.skill, "noop": true}), Meta::default()));
        }

        let message = args
            .message
            .clone()
            .unwrap_or_else(|| format!("save({}): event", args.skill));
        let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;
        let mut meta = Meta::default();
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

    pub fn cmd_snapshot(
        &self,
        args: &SkillOnlyArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let short = gitops::short_head(&self.ctx).map_err(map_git)?;
        let ts = Utc::now().format("%Y%m%dT%H%M%S%fZ");
        let tag = format!("snapshot/{}/{}-{}", args.skill, ts, short);
        gitops::create_annotated_tag(&self.ctx, &tag, &format!("snapshot {}", args.skill))
            .map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "snapshot",
            request_id,
            json!({"skill": args.skill, "tag": tag}),
            &mut meta,
        )?;

        Ok((json!({"skill": args.skill, "tag": tag}), meta))
    }

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

        let tag = format!("release/{}/{}", args.skill, args.version);
        gitops::create_annotated_tag(
            &self.ctx,
            &tag,
            &format!("release {} {}", args.skill, args.version),
        )
        .map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "release",
            request_id,
            json!({"skill": args.skill, "tag": tag}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "version": args.version, "tag": tag}),
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
        gitops::resolve_ref(&self.ctx, &reference).map_err(map_git)?;

        let skill_rel = format!("skills/{}", args.skill);
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

        let message = format!("rollback({}): restore from {}", args.skill, reference);
        let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "rollback",
            request_id,
            json!({"skill": args.skill, "commit": commit, "reference": reference}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "reference": reference, "commit": commit, "noop": false}),
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
