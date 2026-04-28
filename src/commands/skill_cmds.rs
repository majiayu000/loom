use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{AddArgs, CaptureArgs, ImportObservedArgs, ProjectArgs, SaveArgs, SkillOnlyArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;
use crate::state_model::{V3BindingRule, V3ProjectionInstance, V3ProjectionTarget};
use crate::types::ErrorCode;

use super::fs_probe::probe_symlink;
use super::helpers::{
    backup_path_if_exists, copy_dir_recursive, copy_dir_recursive_without_symlinks,
    ensure_skill_exists, map_arg, map_git, map_io, map_lock, map_project_io, map_v3_state,
    maybe_autosync_or_queue, project_skill_to_target, projection_instance_id,
    projection_method_as_str, record_v3_operation, resolve_capture_projection,
    rollback_added_skill, update_projection_after_capture, upsert_projection, upsert_rule,
    validate_projection_method, validate_skill_name,
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

        // Fail-fast physical probe for symlink requests — run BEFORE any
        // destructive operation (backup, remove) so a filesystem that cannot
        // host symlinks (Windows without Developer Mode, FAT32, etc.) does
        // not corrupt an existing projection. Policy allowed it via
        // V3TargetCapabilities; here we verify the filesystem actually can.
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

    pub fn cmd_import_observed(
        &self,
        args: &ImportObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;

        let targets = observed_import_targets(&snapshot.targets.targets, args)?;
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
            let op_id = record_v3_operation(
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
            .map_err(map_v3_state)?;
            meta.op_id = Some(op_id);
            maybe_autosync_or_queue(
                &self.ctx,
                "import-observed",
                request_id,
                json!({"commit": commit, "count": imported.len()}),
                &mut meta,
            )?;
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

fn observed_import_targets(
    targets: &[V3ProjectionTarget],
    args: &ImportObservedArgs,
) -> std::result::Result<Vec<V3ProjectionTarget>, CommandFailure> {
    if let Some(target_id) = args.target.as_deref() {
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
    target: &V3ProjectionTarget,
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

fn rollback_imported_skills(ctx: &crate::state::AppContext, skill_rels: &[String]) {
    for skill_rel in skill_rels {
        let dst = ctx.root.join(skill_rel);
        rollback_added_skill(ctx, skill_rel, &dst);
    }
}
