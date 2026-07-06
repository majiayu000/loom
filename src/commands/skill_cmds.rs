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
    SkillCommitArgs,
};
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::gitops;
#[allow(unused_imports)]
use crate::state_model::{
    RegistryBindingRule, RegistryBindingsFile, RegistryProjectionInstance,
    RegistryProjectionTarget, RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
};
use crate::types::ErrorCode;

use super::file_ops::{
    backup_path_if_exists, copy_dir_recursive_without_symlinks, restore_path_from_backup,
    rollback_added_skill,
};
#[allow(unused_imports)]
use super::fs_probe::probe_symlink;
#[allow(unused_imports)]
use super::helpers::{
    commit_registry_state, ensure_skill_exists, map_arg, map_git, map_io, map_lock, map_project_io,
    map_registry_state, projection_instance_id, projection_method_as_str,
    validate_projection_method, validate_skill_name,
};
#[allow(unused_imports)]
use super::projections::{
    RegistryAuditStateBackup, apply_projection_observation, maybe_autosync_or_queue,
    observe_projection, project_skill_to_target, record_registry_observation,
    record_registry_operation, resolve_capture_projection, restore_registry_audit_state,
    snapshot_registry_audit_state, update_projection_after_capture, upsert_projection, upsert_rule,
};
use super::provenance::{
    provenance_record_for_skill, resolve_add_source, save_record_and_lock, stage_provenance_paths,
};
#[allow(unused_imports)]
use super::skill_safety::enforce_skill_safety;
use super::{App, CommandFailure};

mod commit;
mod observed;
mod save;
pub(crate) mod shared;
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

        let materialized_path = PathBuf::from(&target.path).join(&args.skill);
        let execution = super::projection_executor::execute_projection(
            &self.ctx,
            &paths,
            &snapshot,
            super::projection_executor::ProjectionExecutionInput {
                skill: args.skill.clone(),
                binding,
                binding_is_new: false,
                target,
                target_is_new: false,
                materialized_path,
                method: args.method,
                operation_intent: "skill.project",
                operation_payload: json!({
                    "skill_id": args.skill,
                    "binding_id": args.binding,
                    "target_id": target_id,
                    "method": projection_method_as_str(args.method),
                    "request_id": request_id
                }),
                observation_kind: "projected",
                request_id: request_id.to_string(),
                commit_message: format!("project({}): record projection", args.skill),
                replace_existing: true,
                safe_existing_noop: false,
                after_materialize_fault: Some("skill_project_after_materialize"),
                after_state_save_fault: Some("skill_project_after_state_save"),
                after_observation_fault: Some("skill_project_after_observation"),
                activation_after_projection_fault: false,
            },
        )?;

        let projection = execution.projection.ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "projection executor did not return a projection for skill.project",
            )
        })?;

        Ok((
            json!({
                "projection": projection,
                "backup": execution.backup,
                "commit": execution.commit,
                "noop": execution.noop
            }),
            execution.meta,
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
        if projection.method != crate::core::vocab::ProjectionMethod::Symlink {
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
                let observation = observe_projection(&self.ctx, &projection);
                update_projection_after_capture(
                    &mut projections,
                    &projection.instance_id,
                    &current_rev,
                    Some(&observation),
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
