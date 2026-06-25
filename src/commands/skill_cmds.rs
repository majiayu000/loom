use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::cli::{
    AddArgs, CaptureArgs, ImportObservedArgs, MonitorObservedArgs, ProjectArgs, SaveArgs,
    SkillOnlyArgs,
};
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::gitops;
use crate::state_model::{
    RegistryBindingRule, RegistryBindingsFile, RegistryProjectionInstance,
    RegistryProjectionTarget, RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
};
use crate::types::ErrorCode;

use super::file_ops::{
    backup_path_if_exists, copy_dir_recursive_without_symlinks, restore_path_from_backup,
    rollback_added_skill,
};
use super::fs_probe::probe_symlink;
use super::helpers::{
    commit_registry_state, ensure_skill_exists, map_arg, map_git, map_io, map_lock, map_project_io,
    map_registry_state, projection_instance_id, projection_method_as_str,
    validate_projection_method, validate_skill_name,
};
use super::projections::{
    RegistryAuditStateBackup, maybe_autosync_or_queue, project_skill_to_target,
    record_registry_observation, record_registry_operation, resolve_capture_projection,
    restore_registry_audit_state, snapshot_registry_audit_state, update_projection_after_capture,
    upsert_projection, upsert_rule,
};
use super::provenance::{
    provenance_record_for_skill, resolve_add_source, save_record_and_lock, stage_provenance_paths,
};
use super::{App, CommandFailure};

mod observed;
mod save;
mod shared;
mod snapshot;

use self::shared::*;

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
        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        let source = resolve_add_source(&self.ctx, args, &staging_root)?;
        if let Err(err) = copy_dir_recursive_without_symlinks(&source.copy_source, &staging_skill) {
            cleanup_staging();
            return Err(map_io(err));
        }

        if let Err(err) = fs::rename(&staging_skill, &dst) {
            cleanup_staging();
            return Err(map_io(err));
        }
        cleanup_staging();

        let mut meta = Meta::default();
        let skill_rel = format!("skills/{}", args.name);
        let provenance = provenance_record_for_skill(&args.name, source.descriptor, &dst)?;
        if let Err(err) = save_record_and_lock(&self.ctx, provenance) {
            rollback_added_skill(&self.ctx, &skill_rel, &dst);
            return Err(err);
        }
        if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
            rollback_added_skill(&self.ctx, &skill_rel, &dst);
            return Err(map_git(err));
        }
        if let Err(err) = stage_provenance_paths(&self.ctx) {
            rollback_added_skill(&self.ctx, &skill_rel, &dst);
            return Err(err);
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

        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
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

        if target.agent != binding.agent {
            return Err(CommandFailure::new(
                ErrorCode::TargetAgentMismatch,
                format!(
                    "binding '{}' is for agent '{}' but target '{}' is for agent '{}'",
                    binding.binding_id, binding.agent, target.target_id, target.agent
                ),
            ));
        }

        if target.ownership != "managed" {
            return Err(CommandFailure::new(
                ErrorCode::TargetNotManaged,
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

        // Fail-fast physical probe for symlink requests — run BEFORE any
        // destructive operation (backup, remove) so a filesystem that cannot
        // host symlinks (Windows without Developer Mode, FAT32, etc.) does
        // not corrupt an existing projection. Policy allowed it via
        // RegistryTargetCapabilities; here we verify the filesystem actually can.
        if matches!(args.method, crate::cli::ProjectionMethod::Symlink) {
            let probe = probe_symlink(&target_base);
            if !probe.supported {
                return Err(CommandFailure::new(
                    ErrorCode::ProjectionMethodUnsupported,
                    format!(
                        "target '{}' filesystem does not support symlink projection: {}. \
                         retry with --method copy",
                        target.target_id,
                        probe.reason.unwrap_or_else(|| "unknown reason".to_string())
                    ),
                ));
            }
        }

        let replaced_projection_backup =
            backup_path_if_exists(&self.ctx, &materialized_path, "project.replace_projection")
                .map_err(map_io)?;
        if let Err(err) = remove_path_if_exists(&materialized_path) {
            let rollback_errors = rollback_project_mutation(
                &paths,
                &materialized_path,
                replaced_projection_backup.as_ref(),
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            );
            return Err(map_io(err).with_rollback_errors(rollback_errors));
        }
        if let Err(err) = project_skill_to_target(&skill_src, &materialized_path, args.method) {
            let rollback_errors = rollback_project_mutation(
                &paths,
                &materialized_path,
                replaced_projection_backup.as_ref(),
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            );
            return Err(map_project_io(args.method)(err).with_rollback_errors(rollback_errors));
        }

        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let mut rules = original_rules.clone();
        upsert_rule(
            &mut rules,
            RegistryBindingRule {
                binding_id: binding.binding_id.clone(),
                skill_id: args.skill.clone(),
                target_id: target.target_id.clone(),
                method: projection_method_as_str(args.method).to_string(),
                watch_policy: "observe_only".to_string(),
                created_at: Some(Utc::now()),
            },
        );

        let mut projections = original_projections.clone();
        let instance_id =
            projection_instance_id(&args.skill, &binding.binding_id, &target.target_id);
        let projection = RegistryProjectionInstance {
            instance_id: instance_id.clone(),
            skill_id: args.skill.clone(),
            binding_id: Some(binding.binding_id.clone()),
            target_id: target.target_id.clone(),
            materialized_path: materialized_path.display().to_string(),
            method: projection_method_as_str(args.method).to_string(),
            last_applied_rev: gitops::head(&self.ctx).map_err(map_git)?,
            health: "healthy".to_string(),
            observed_drift: Some(false),
            updated_at: Some(Utc::now()),
        };
        upsert_projection(&mut projections, projection.clone());

        let registry_audit_backup =
            snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let post_materialize: std::result::Result<(Option<String>, Meta), CommandFailure> =
            (|| {
                maybe_skill_fault("skill_project_after_materialize")?;
                paths
                    .save_bindings_rules_projections(&original_bindings, &rules, &projections)
                    .map_err(map_registry_state)?;
                maybe_skill_fault("skill_project_after_state_save")?;
                let op_id = record_registry_operation(
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
                .map_err(map_registry_state)?;
                record_registry_observation(
                    &paths,
                    &instance_id,
                    "projected",
                    Some(projection.materialized_path.clone()),
                    None,
                    Some(projection.last_applied_rev.clone()),
                )
                .map_err(map_registry_state)?;
                maybe_skill_fault("skill_project_after_observation")?;
                let commit = commit_registry_state(
                    &self.ctx,
                    &format!("project({}): record projection", args.skill),
                )?;
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                if let Some(commit) = &commit {
                    maybe_autosync_or_queue(
                        &self.ctx,
                        "skill.project",
                        request_id,
                        json!({
                            "skill": args.skill,
                            "binding_id": binding.binding_id,
                            "target_id": target.target_id,
                            "commit": commit
                        }),
                        &mut meta,
                    )?;
                }
                Ok((commit, meta))
            })();

        let (commit, meta) = match post_materialize {
            Ok(result) => result,
            Err(err) => {
                let mut rollback_errors = Vec::new();
                if let Err(restore_err) =
                    restore_registry_audit_state(&paths, &registry_audit_backup)
                {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_audit_state",
                        restore_err,
                    );
                }
                rollback_errors.extend(rollback_project_mutation(
                    &paths,
                    &materialized_path,
                    replaced_projection_backup.as_ref(),
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                ));
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };

        Ok((
            json!({"projection": projection, "backup": replaced_projection_backup, "commit": commit, "noop": false}),
            meta,
        ))
    }

    pub fn cmd_capture(
        &self,
        args: &CaptureArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
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

        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let previous_index = gitops::snapshot_index(&self.ctx).map_err(map_git)?;
        let mut source_backup = None;
        let mut source_replaced = false;
        if projection.method != "symlink" {
            ensure_capture_source_not_drifted(&self.ctx, &projection, Path::new(&skill_rel))?;
            let tmp_path = self
                .ctx
                .state_dir
                .join(format!("tmp-capture-{}", Uuid::new_v4()));
            let _ = remove_path_if_exists(&tmp_path);
            if let Err(err) = copy_dir_recursive_without_symlinks(&live_path, &tmp_path) {
                let _ = remove_path_if_exists(&tmp_path);
                return Err(map_io(err));
            }
            source_backup =
                match backup_path_if_exists(&self.ctx, &skill_path, "capture.replace_source") {
                    Ok(backup) => backup,
                    Err(err) => {
                        let _ = remove_path_if_exists(&tmp_path);
                        return Err(map_io(err));
                    }
                };
            if let Err(err) = remove_path_if_exists(&skill_path) {
                let mut rollback_errors = rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    true,
                    &previous_head,
                    &previous_index,
                    false,
                );
                if let Err(restore_err) = rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                ) {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_state",
                        restore_err,
                    );
                }
                let _ = remove_path_if_exists(&tmp_path);
                return Err(map_io(err).with_rollback_errors(rollback_errors));
            }
            if let Err(err) = fs::rename(&tmp_path, &skill_path) {
                let mut rollback_errors = rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    true,
                    &previous_head,
                    &previous_index,
                    false,
                );
                if let Err(restore_err) = rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                ) {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_state",
                        restore_err,
                    );
                }
                let _ = remove_path_if_exists(&tmp_path);
                return Err(map_io(err).with_rollback_errors(rollback_errors));
            }
            source_replaced = true;
        }

        let mut commit_created = false;
        let registry_audit_backup =
            snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let post_replace: std::result::Result<(Option<String>, String, bool), CommandFailure> =
            (|| {
                maybe_skill_fault("skill_capture_after_source_replace")?;
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
                    let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;
                    commit_created = true;
                    Some(commit)
                } else {
                    None
                };
                maybe_skill_fault("skill_capture_after_commit")?;
                let current_rev = gitops::head(&self.ctx).map_err(map_git)?;

                let mut projections = original_projections.clone();
                update_projection_after_capture(
                    &mut projections,
                    &projection.instance_id,
                    &current_rev,
                )?;
                paths
                    .save_bindings_rules_projections(
                        &original_bindings,
                        &original_rules,
                        &projections,
                    )
                    .map_err(map_registry_state)?;
                maybe_skill_fault("skill_capture_after_state_save")?;

                let op_id = record_registry_operation(
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
                .map_err(map_registry_state)?;
                record_registry_observation(
                    &paths,
                    &projection.instance_id,
                    "captured",
                    Some(live_path.display().to_string()),
                    Some(projection.last_applied_rev.clone()),
                    Some(current_rev),
                )
                .map_err(map_registry_state)?;
                maybe_skill_fault("skill_capture_after_observation")?;
                Ok((commit, op_id, changed))
            })();

        let (commit, op_id, changed) = match post_replace {
            Ok(result) => result,
            Err(err) => {
                let mut rollback_errors = Vec::new();
                if let Err(restore_err) =
                    restore_registry_audit_state(&paths, &registry_audit_backup)
                {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_audit_state",
                        restore_err,
                    );
                }
                rollback_errors.extend(rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    source_replaced,
                    &previous_head,
                    &previous_index,
                    commit_created,
                ));
                if let Err(restore_err) = rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                ) {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_state",
                        restore_err,
                    );
                }
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };

        let mut state_commit_created = false;
        let post_state_commit: std::result::Result<(Option<String>, Meta), CommandFailure> =
            (|| {
                let state_commit = commit_registry_state(
                    &self.ctx,
                    &format!("capture({}): record registry state", projection.skill_id),
                )?;
                if state_commit.is_some() {
                    state_commit_created = true;
                }
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                if commit.is_some() || state_commit.is_some() {
                    maybe_autosync_or_queue(
                        &self.ctx,
                        "skill.capture",
                        request_id,
                        json!({
                            "skill": projection.skill_id,
                            "instance_id": projection.instance_id,
                            "commit": commit,
                            "state_commit": state_commit
                        }),
                        &mut meta,
                    )?;
                }
                Ok((state_commit, meta))
            })();

        let (state_commit, meta) = match post_state_commit {
            Ok(result) => result,
            Err(err) => {
                let mut rollback_errors = Vec::new();
                if let Err(restore_err) =
                    restore_registry_audit_state(&paths, &registry_audit_backup)
                {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_audit_state",
                        restore_err,
                    );
                }
                rollback_errors.extend(rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    source_replaced,
                    &previous_head,
                    &previous_index,
                    commit_created || state_commit_created,
                ));
                if let Err(restore_err) = rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                ) {
                    push_rollback_error(
                        &mut rollback_errors,
                        "restore_registry_state",
                        restore_err,
                    );
                }
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };

        Ok((
            json!({
                "capture": {
                    "skill_id": projection.skill_id,
                    "binding_id": projection.binding_id,
                    "instance_id": projection.instance_id,
                    "commit": commit,
                    "state_commit": state_commit,
                    "backup": source_backup,
                    "noop": !changed
                }
            }),
            meta,
        ))
    }
}
