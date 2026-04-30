use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::cli::{
    AddArgs, CaptureArgs, ImportObservedArgs, MonitorObservedArgs, ProjectArgs, SaveArgs,
    SkillOnlyArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;
use crate::state_model::{
    RegistryBindingRule, RegistryBindingsFile, RegistryProjectionInstance,
    RegistryProjectionTarget, RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
};
use crate::types::ErrorCode;

use super::fs_probe::probe_symlink;
use super::helpers::{
    backup_path_if_exists, commit_registry_state, copy_dir_recursive,
    copy_dir_recursive_without_symlinks, ensure_skill_exists, map_arg, map_git, map_io, map_lock,
    map_project_io, map_registry_state, maybe_autosync_or_queue, project_skill_to_target,
    projection_instance_id, projection_method_as_str, record_registry_operation,
    resolve_capture_projection, restore_path_from_backup, rollback_added_skill,
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

        // Fail-fast physical probe for symlink requests — run BEFORE any
        // destructive operation (backup, remove) so a filesystem that cannot
        // host symlinks (Windows without Developer Mode, FAT32, etc.) does
        // not corrupt an existing projection. Policy allowed it via
        // RegistryTargetCapabilities; here we verify the filesystem actually can.
        if matches!(args.method, crate::cli::ProjectionMethod::Symlink) {
            let probe = probe_symlink(&target_base);
            if !probe.supported {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
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
            rollback_project_mutation(
                &paths,
                &materialized_path,
                replaced_projection_backup.as_ref(),
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            );
            return Err(map_io(err));
        }
        if let Err(err) = project_skill_to_target(&skill_src, &materialized_path, args.method) {
            rollback_project_mutation(
                &paths,
                &materialized_path,
                replaced_projection_backup.as_ref(),
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            );
            return Err(map_project_io(args.method)(err));
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
                rollback_project_mutation(
                    &paths,
                    &materialized_path,
                    replaced_projection_backup.as_ref(),
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                );
                return Err(err);
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
        let mut source_backup = None;
        let mut source_replaced = false;
        if projection.method != "symlink" {
            let tmp_path = self
                .ctx
                .state_dir
                .join(format!("tmp-capture-{}", Uuid::new_v4()));
            let _ = remove_path_if_exists(&tmp_path);
            if let Err(err) = copy_dir_recursive(&live_path, &tmp_path) {
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
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_rel,
                    &skill_path,
                    source_backup.as_ref(),
                    true,
                    &previous_head,
                    false,
                );
                rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                );
                let _ = remove_path_if_exists(&tmp_path);
                return Err(map_io(err));
            }
            if let Err(err) = fs::rename(&tmp_path, &skill_path) {
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_rel,
                    &skill_path,
                    source_backup.as_ref(),
                    true,
                    &previous_head,
                    false,
                );
                rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                );
                let _ = remove_path_if_exists(&tmp_path);
                return Err(map_io(err));
            }
            source_replaced = true;
        }

        let mut commit_created = false;
        let post_replace = (|| {
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
                .save_bindings_rules_projections(&original_bindings, &original_rules, &projections)
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
            Ok((commit, op_id, changed))
        })();

        let (commit, op_id, changed) = match post_replace {
            Ok(result) => result,
            Err(err) => {
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_rel,
                    &skill_path,
                    source_backup.as_ref(),
                    source_replaced,
                    &previous_head,
                    commit_created,
                );
                rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                );
                return Err(err);
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
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_rel,
                    &skill_path,
                    source_backup.as_ref(),
                    source_replaced,
                    &previous_head,
                    commit_created || state_commit_created,
                );
                rollback_registry_state(
                    &paths,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                );
                return Err(err);
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

    pub fn cmd_import_observed(
        &self,
        args: &ImportObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;

        let targets = observed_import_targets(&snapshot.targets.targets, args.target.as_deref())?;
        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-import-observed-{}", Uuid::new_v4()));
        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        let mut imported_rels = Vec::new();

        for target in targets {
            let target_path = PathBuf::from(&target.path);
            if !target_path.exists() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-missing",
                }));
                continue;
            }
            if !target_path.is_dir() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-not-directory",
                }));
                continue;
            }

            let mut entries = match fs::read_dir(&target_path) {
                Ok(entries) => entries
                    .filter_map(|entry| match entry {
                        Ok(entry) => Some(entry),
                        Err(err) => {
                            skipped.push(json!({
                                "target_id": target.target_id,
                                "path": target.path,
                                "reason": "entry-read-failed",
                                "error": err.to_string(),
                            }));
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                Err(err) => {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "path": target.path,
                        "reason": "target-read-failed",
                        "error": err.to_string(),
                    }));
                    continue;
                }
            };
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let source_path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(err) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "reason": "file-type-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                };
                let (copy_source, source_kind, resolved_source) = match observed_skill_copy_source(
                    &source_path,
                    &file_type,
                    &mut skipped,
                    &target,
                ) {
                    Some(source) => source,
                    None => continue,
                };
                if !has_skill_entrypoint(&copy_source) {
                    continue;
                }

                let skill_id = match entry.file_name().into_string() {
                    Ok(name) => name,
                    Err(name) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "name": name.to_string_lossy(),
                            "reason": "non-utf8-name",
                        }));
                        continue;
                    }
                };

                if let Err(err) = validate_skill_name(&skill_id) {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "invalid-skill-name",
                        "error": err.to_string(),
                    }));
                    continue;
                }

                let dst = self.ctx.skill_path(&skill_id);
                if dst.exists() {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "already-exists",
                    }));
                    continue;
                }

                let staging_skill = staging_root.join(&skill_id);
                let _ = remove_path_if_exists(&staging_skill);
                match copy_dir_recursive_without_symlinks(&copy_source, &staging_skill) {
                    Ok(()) => {}
                    Err(err) => {
                        let _ = remove_path_if_exists(&staging_skill);
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "skill": skill_id,
                            "source": source_path.display().to_string(),
                            "reason": "copy-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                }

                if let Err(err) = fs::rename(&staging_skill, &dst) {
                    cleanup_staging();
                    rollback_imported_skills(&self.ctx, &imported_rels);
                    return Err(map_io(err));
                }

                let skill_rel = format!("skills/{}", skill_id);
                if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
                    cleanup_staging();
                    rollback_imported_skills(&self.ctx, &imported_rels);
                    rollback_added_skill(&self.ctx, &skill_rel, &dst);
                    return Err(map_git(err));
                }
                imported_rels.push(skill_rel);
                let mut imported_item = json!({
                    "target_id": target.target_id,
                    "skill": skill_id,
                    "source": source_path.display().to_string(),
                    "source_kind": source_kind,
                    "path": dst.display().to_string(),
                });
                if let Some(resolved_source) = resolved_source {
                    imported_item["resolved_source"] = json!(resolved_source);
                }
                imported.push(imported_item);
            }
        }

        cleanup_staging();

        let mut meta = Meta::default();
        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let registry_backup =
            snapshot_registry_operation_state(&paths).map_err(map_registry_state)?;
        let mut changed = false;
        for skill_rel in &imported_rels {
            match gitops::has_staged_changes_for_path(&self.ctx, Path::new(skill_rel)) {
                Ok(true) => {
                    changed = true;
                    break;
                }
                Ok(false) => {}
                Err(err) => {
                    rollback_imported_skills(&self.ctx, &imported_rels);
                    return Err(map_git(err));
                }
            }
        }

        let commit = if changed {
            let message = match imported.len() {
                1 => format!(
                    "import-observed({}): from observed target",
                    imported[0]["skill"].as_str().unwrap_or("skill")
                ),
                count => format!("import-observed: {} skills", count),
            };
            let commit = match gitops::commit(&self.ctx, &message) {
                Ok(commit) => commit,
                Err(err) => {
                    rollback_imported_skills(&self.ctx, &imported_rels);
                    return Err(map_git(err));
                }
            };
            let post_commit = (|| -> std::result::Result<Meta, CommandFailure> {
                let op_id = record_registry_operation(
                    &paths,
                    "skill.import_observed",
                    json!({
                        "target": args.target,
                        "request_id": request_id
                    }),
                    json!({
                        "commit": commit,
                        "imported": imported,
                        "skipped": skipped
                    }),
                )
                .map_err(map_registry_state)?;
                let state_commit =
                    commit_registry_state(&self.ctx, "import-observed: record registry state")?;
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                maybe_autosync_or_queue(
                    &self.ctx,
                    "import-observed",
                    request_id,
                    json!({"commit": commit, "state_commit": state_commit, "count": imported.len()}),
                    &mut meta,
                )?;
                Ok(meta)
            })();
            let post_meta = match post_commit {
                Ok(result) => result,
                Err(err) => {
                    rollback_import_after_commit(
                        &self.ctx,
                        &paths,
                        &registry_backup,
                        &previous_head,
                        &imported_rels,
                    );
                    return Err(err);
                }
            };
            meta = post_meta;
            Some(commit)
        } else {
            None
        };

        Ok((
            json!({
                "count": imported.len(),
                "imported": imported,
                "skipped": skipped,
                "commit": commit,
                "noop": !changed,
            }),
            meta,
        ))
    }

    pub fn cmd_monitor_observed(
        &self,
        args: &MonitorObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        if !args.once && args.interval_seconds == 0 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--interval-seconds must be greater than 0 for long-running monitoring",
            ));
        }

        let mut cycles = 0_u64;
        let mut totals = MonitorTotals::default();
        let mut last_cycle = json!(null);
        let mut meta = Meta::default();

        loop {
            let (cycle, cycle_meta) = self.monitor_observed_once(args, request_id)?;
            cycles += 1;
            totals.add_cycle(&cycle);
            last_cycle = cycle;
            merge_monitor_meta(&mut meta, cycle_meta);

            if args.once || args.max_cycles.is_some_and(|max| cycles >= max) {
                break;
            }

            thread::sleep(Duration::from_secs(args.interval_seconds));
        }

        Ok((
            json!({
                "cycles": cycles,
                "totals": totals.to_json(),
                "last_cycle": last_cycle,
            }),
            meta,
        ))
    }

    fn monitor_observed_once(
        &self,
        args: &MonitorObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let targets = observed_import_targets(&snapshot.targets.targets, args.target.as_deref())?;
        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-monitor-observed-{}", Uuid::new_v4()));
        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        let mut imported = Vec::new();
        let mut updated = Vec::new();
        let mut skipped = Vec::new();
        let mut unchanged_count = 0_usize;
        let mut changed_rels = Vec::new();
        let mut imported_rels = Vec::new();
        let mut update_rollbacks = Vec::new();
        let mut seen_skill_ids = BTreeSet::new();

        for target in targets {
            let target_path = PathBuf::from(&target.path);
            if !target_path.exists() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-missing",
                }));
                continue;
            }
            if !target_path.is_dir() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-not-directory",
                }));
                continue;
            }

            let mut entries = match fs::read_dir(&target_path) {
                Ok(entries) => entries
                    .filter_map(|entry| match entry {
                        Ok(entry) => Some(entry),
                        Err(err) => {
                            skipped.push(json!({
                                "target_id": target.target_id,
                                "path": target.path,
                                "reason": "entry-read-failed",
                                "error": err.to_string(),
                            }));
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                Err(err) => {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "path": target.path,
                        "reason": "target-read-failed",
                        "error": err.to_string(),
                    }));
                    continue;
                }
            };
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let source_path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(err) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "reason": "file-type-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                };
                let (copy_source, source_kind, resolved_source) = match observed_skill_copy_source(
                    &source_path,
                    &file_type,
                    &mut skipped,
                    &target,
                ) {
                    Some(source) => source,
                    None => continue,
                };
                if !has_skill_entrypoint(&copy_source) {
                    continue;
                }

                let skill_id = match entry.file_name().into_string() {
                    Ok(name) => name,
                    Err(name) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "name": name.to_string_lossy(),
                            "reason": "non-utf8-name",
                        }));
                        continue;
                    }
                };

                if let Err(err) = validate_skill_name(&skill_id) {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "invalid-skill-name",
                        "error": err.to_string(),
                    }));
                    continue;
                }

                if !seen_skill_ids.insert(skill_id.clone()) {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "duplicate-observed-skill",
                    }));
                    continue;
                }

                let staging_skill = staging_root.join("next").join(&skill_id);
                let _ = remove_path_if_exists(&staging_skill);
                match copy_dir_recursive_without_symlinks(&copy_source, &staging_skill) {
                    Ok(()) => {}
                    Err(err) => {
                        let _ = remove_path_if_exists(&staging_skill);
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "skill": skill_id,
                            "source": source_path.display().to_string(),
                            "reason": "copy-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                }

                let dst = self.ctx.skill_path(&skill_id);
                let skill_rel = format!("skills/{}", skill_id);
                let mut item = json!({
                    "target_id": target.target_id,
                    "skill": skill_id,
                    "source": source_path.display().to_string(),
                    "source_kind": source_kind,
                    "path": dst.display().to_string(),
                });
                if let Some(resolved_source) = resolved_source {
                    item["resolved_source"] = json!(resolved_source);
                }

                if dst.exists() {
                    match materialized_dirs_equal(&dst, &staging_skill) {
                        Ok(true) => {
                            unchanged_count += 1;
                            let _ = remove_path_if_exists(&staging_skill);
                            continue;
                        }
                        Ok(false) => {}
                        Err(err) => {
                            let _ = remove_path_if_exists(&staging_skill);
                            skipped.push(json!({
                                "target_id": target.target_id,
                                "skill": item["skill"].clone(),
                                "source": source_path.display().to_string(),
                                "reason": "compare-failed",
                                "error": err.to_string(),
                            }));
                            continue;
                        }
                    }

                    let previous = staging_root.join("previous").join(
                        item["skill"]
                            .as_str()
                            .expect("monitor item skill is string"),
                    );
                    if let Some(parent) = previous.parent() {
                        fs::create_dir_all(parent).map_err(map_io)?;
                    }
                    if let Err(err) = fs::rename(&dst, &previous) {
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_io(err));
                    }
                    if let Err(err) = fs::rename(&staging_skill, &dst) {
                        let _ = fs::rename(&previous, &dst);
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_io(err));
                    }
                    if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
                        let _ = remove_path_if_exists(&dst);
                        let _ = fs::rename(&previous, &dst);
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_git(err));
                    }
                    update_rollbacks.push(MonitorUpdateRollback {
                        skill_rel: skill_rel.clone(),
                        dst,
                        previous,
                    });
                    changed_rels.push(skill_rel);
                    updated.push(item);
                } else {
                    if let Err(err) = fs::rename(&staging_skill, &dst) {
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_io(err));
                    }
                    if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
                        rollback_added_skill(&self.ctx, &skill_rel, &dst);
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_git(err));
                    }
                    imported_rels.push(skill_rel.clone());
                    changed_rels.push(skill_rel);
                    imported.push(item);
                }
            }
        }

        let mut has_changes = false;
        for skill_rel in &changed_rels {
            match gitops::has_staged_changes_for_path(&self.ctx, Path::new(skill_rel)) {
                Ok(true) => {
                    has_changes = true;
                    break;
                }
                Ok(false) => {}
                Err(err) => {
                    rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                    cleanup_staging();
                    return Err(map_git(err));
                }
            }
        }

        let mut meta = Meta::default();
        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let registry_backup =
            snapshot_registry_operation_state(&paths).map_err(map_registry_state)?;
        let commit = if has_changes {
            let change_count = imported.len() + updated.len();
            let message = if change_count == 1 {
                let skill = imported
                    .first()
                    .or_else(|| updated.first())
                    .and_then(|item| item["skill"].as_str())
                    .unwrap_or("skill");
                format!("monitor-observed({}): sync observed skill", skill)
            } else {
                format!("monitor-observed: {} skills", change_count)
            };
            let commit = match gitops::commit(&self.ctx, &message) {
                Ok(commit) => commit,
                Err(err) => {
                    rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                    cleanup_staging();
                    return Err(map_git(err));
                }
            };
            let post_commit = (|| -> std::result::Result<Meta, CommandFailure> {
                let op_id = record_registry_operation(
                    &paths,
                    "skill.monitor_observed",
                    json!({
                        "target": args.target,
                        "request_id": request_id
                    }),
                    json!({
                        "commit": commit,
                        "imported": imported,
                        "updated": updated,
                        "skipped": skipped,
                        "unchanged_count": unchanged_count,
                    }),
                )
                .map_err(map_registry_state)?;
                let state_commit =
                    commit_registry_state(&self.ctx, "monitor-observed: record registry state")?;
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                maybe_autosync_or_queue(
                    &self.ctx,
                    "monitor-observed",
                    request_id,
                    json!({
                        "commit": commit,
                        "state_commit": state_commit,
                        "imported": imported.len(),
                        "updated": updated.len(),
                    }),
                    &mut meta,
                )?;
                Ok(meta)
            })();
            let post_meta = match post_commit {
                Ok(result) => result,
                Err(err) => {
                    rollback_monitor_after_commit(
                        &self.ctx,
                        &paths,
                        &registry_backup,
                        &previous_head,
                        &imported_rels,
                        &update_rollbacks,
                    );
                    cleanup_staging();
                    return Err(err);
                }
            };
            meta = post_meta;
            Some(commit)
        } else {
            None
        };

        cleanup_staging();
        let change_count = imported.len() + updated.len();
        Ok((
            json!({
                "count": change_count,
                "imported_count": imported.len(),
                "updated_count": updated.len(),
                "unchanged_count": unchanged_count,
                "skipped_count": skipped.len(),
                "imported": imported,
                "updated": updated,
                "skipped": skipped,
                "commit": commit,
                "noop": !has_changes,
            }),
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
}

fn maybe_skill_fault(tag: &str) -> std::result::Result<(), CommandFailure> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            format!("fault injected at {}", tag),
        ));
    }
    Ok(())
}

fn rollback_project_mutation(
    paths: &RegistryStatePaths,
    materialized_path: &Path,
    backup: Option<&serde_json::Value>,
    original_bindings: &RegistryBindingsFile,
    original_rules: &RegistryRulesFile,
    original_projections: &RegistryProjectionsFile,
) {
    if let Some(backup) = backup {
        let _ = restore_path_from_backup(materialized_path, backup);
    } else {
        let _ = remove_path_if_exists(materialized_path);
    }
    rollback_registry_state(
        paths,
        original_bindings,
        original_rules,
        original_projections,
    );
}

fn rollback_capture_mutation(
    ctx: &crate::state::AppContext,
    skill_rel: &str,
    skill_path: &Path,
    source_backup: Option<&serde_json::Value>,
    source_replaced: bool,
    previous_head: &str,
    commit_created: bool,
) {
    if commit_created {
        // Preserve worktree content while removing only the command-created commit.
        let _ = gitops::run_git_allow_failure(ctx, &["reset", "--soft", previous_head]);
    }

    if source_replaced {
        if let Some(backup) = source_backup {
            let _ = restore_path_from_backup(skill_path, backup);
        } else {
            let _ = remove_path_if_exists(skill_path);
        }
    }

    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", skill_rel]);
}

fn rollback_registry_state(
    paths: &RegistryStatePaths,
    original_bindings: &RegistryBindingsFile,
    original_rules: &RegistryRulesFile,
    original_projections: &RegistryProjectionsFile,
) {
    let _ = paths.save_bindings_rules_projections(
        original_bindings,
        original_rules,
        original_projections,
    );
}

#[derive(Debug, Default)]
struct MonitorTotals {
    imported: usize,
    updated: usize,
    unchanged: usize,
    skipped: usize,
}

impl MonitorTotals {
    fn add_cycle(&mut self, cycle: &serde_json::Value) {
        self.imported += cycle["imported_count"].as_u64().unwrap_or(0) as usize;
        self.updated += cycle["updated_count"].as_u64().unwrap_or(0) as usize;
        self.unchanged += cycle["unchanged_count"].as_u64().unwrap_or(0) as usize;
        self.skipped += cycle["skipped_count"].as_u64().unwrap_or(0) as usize;
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "imported": self.imported,
            "updated": self.updated,
            "unchanged": self.unchanged,
            "skipped": self.skipped,
            "changed": self.imported + self.updated,
        })
    }
}

#[derive(Debug)]
struct MonitorUpdateRollback {
    skill_rel: String,
    dst: PathBuf,
    previous: PathBuf,
}

fn merge_monitor_meta(meta: &mut Meta, cycle_meta: Meta) {
    if cycle_meta.op_id.is_some() {
        meta.op_id = cycle_meta.op_id;
    }
    if cycle_meta.sync_state.is_some() {
        meta.sync_state = cycle_meta.sync_state;
    }
    meta.warnings.extend(cycle_meta.warnings);
}

fn observed_import_targets(
    targets: &[RegistryProjectionTarget],
    target_id: Option<&str>,
) -> std::result::Result<Vec<RegistryProjectionTarget>, CommandFailure> {
    if let Some(target_id) = target_id {
        let target = targets
            .iter()
            .find(|target| target.target_id == target_id)
            .cloned()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::TargetNotFound,
                    format!("target '{}' not found", target_id),
                )
            })?;
        if target.ownership != "observed" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' has ownership '{}' and cannot be imported as observed",
                    target.target_id, target.ownership
                ),
            ));
        }
        return Ok(vec![target]);
    }

    Ok(targets
        .iter()
        .filter(|target| target.ownership == "observed")
        .cloned()
        .collect())
}

fn observed_skill_copy_source(
    source_path: &Path,
    file_type: &fs::FileType,
    skipped: &mut Vec<serde_json::Value>,
    target: &RegistryProjectionTarget,
) -> Option<(PathBuf, &'static str, Option<String>)> {
    if file_type.is_dir() {
        return Some((source_path.to_path_buf(), "directory", None));
    }
    if !file_type.is_symlink() {
        return None;
    }

    let metadata = match fs::metadata(source_path) {
        Ok(metadata) => metadata,
        Err(err) => {
            skipped.push(json!({
                "target_id": target.target_id.clone(),
                "source": source_path.display().to_string(),
                "reason": "symlink-target-failed",
                "error": err.to_string(),
            }));
            return None;
        }
    };
    if !metadata.is_dir() {
        return None;
    }

    match fs::canonicalize(source_path) {
        Ok(resolved) => {
            let display = resolved.display().to_string();
            Some((resolved, "symlink", Some(display)))
        }
        Err(err) => {
            skipped.push(json!({
                "target_id": target.target_id.clone(),
                "source": source_path.display().to_string(),
                "reason": "symlink-resolve-failed",
                "error": err.to_string(),
            }));
            None
        }
    }
}

fn has_skill_entrypoint(path: &Path) -> bool {
    path.join("SKILL.md").is_file() || path.join("skill.md").is_file()
}

fn materialized_dirs_equal(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let left_files = collect_materialized_files(left)?;
    let right_files = collect_materialized_files(right)?;
    if left_files.len() != right_files.len() {
        return Ok(false);
    }

    for (rel, left_body) in left_files {
        match right_files.get(&rel) {
            Some(right_body) if right_body == &left_body => {}
            _ => return Ok(false),
        }
    }

    Ok(true)
}

fn collect_materialized_files(root: &Path) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", root.display()))?;
        let rel = entry.path().strip_prefix(root).with_context(|| {
            format!(
                "failed to derive relative path for {} under {}",
                entry.path().display(),
                root.display()
            )
        })?;
        if rel.as_os_str().is_empty() || entry.file_type().is_dir() {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Ok(BTreeMap::from([(
                rel.to_path_buf(),
                b"__loom_symlink_marker__".to_vec(),
            )]));
        }
        if entry.file_type().is_file() {
            let body = fs::read(entry.path())
                .with_context(|| format!("failed to read {}", entry.path().display()))?;
            files.insert(rel.to_path_buf(), body);
        }
    }

    Ok(files)
}

struct RegistryOperationStateBackup {
    operations: Vec<u8>,
    checkpoint: Vec<u8>,
}

fn snapshot_registry_operation_state(
    paths: &RegistryStatePaths,
) -> anyhow::Result<RegistryOperationStateBackup> {
    Ok(RegistryOperationStateBackup {
        operations: fs::read(&paths.operations_file)
            .with_context(|| format!("failed to snapshot {}", paths.operations_file.display()))?,
        checkpoint: fs::read(&paths.checkpoint_file)
            .with_context(|| format!("failed to snapshot {}", paths.checkpoint_file.display()))?,
    })
}

fn restore_registry_operation_state(
    paths: &RegistryStatePaths,
    backup: &RegistryOperationStateBackup,
) -> anyhow::Result<()> {
    fs::write(&paths.operations_file, &backup.operations)
        .with_context(|| format!("failed to restore {}", paths.operations_file.display()))?;
    fs::write(&paths.checkpoint_file, &backup.checkpoint)
        .with_context(|| format!("failed to restore {}", paths.checkpoint_file.display()))?;
    Ok(())
}

fn reset_command_created_commits(ctx: &crate::state::AppContext, previous_head: &str) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "--soft", previous_head]);
}

fn unstage_registry_state(ctx: &crate::state::AppContext) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/registry"]);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/v3"]);
}

fn rollback_import_after_commit(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryOperationStateBackup,
    previous_head: &str,
    imported_rels: &[String],
) {
    reset_command_created_commits(ctx, previous_head);
    rollback_imported_skills(ctx, imported_rels);
    let _ = restore_registry_operation_state(paths, registry_backup);
    unstage_registry_state(ctx);
}

fn rollback_monitor_after_commit(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryOperationStateBackup,
    previous_head: &str,
    imported_rels: &[String],
    update_rollbacks: &[MonitorUpdateRollback],
) {
    reset_command_created_commits(ctx, previous_head);
    rollback_monitor_changes(ctx, imported_rels, update_rollbacks);
    let _ = restore_registry_operation_state(paths, registry_backup);
    unstage_registry_state(ctx);
}

fn rollback_monitor_changes(
    ctx: &crate::state::AppContext,
    imported_rels: &[String],
    update_rollbacks: &[MonitorUpdateRollback],
) {
    for update in update_rollbacks.iter().rev() {
        let _ = remove_path_if_exists(&update.dst);
        let _ = fs::rename(&update.previous, &update.dst);
        let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", &update.skill_rel]);
    }

    rollback_imported_skills(ctx, imported_rels);
}

fn rollback_imported_skills(ctx: &crate::state::AppContext, skill_rels: &[String]) {
    for skill_rel in skill_rels {
        let dst = ctx.root.join(skill_rel);
        rollback_added_skill(ctx, skill_rel, &dst);
    }
}
