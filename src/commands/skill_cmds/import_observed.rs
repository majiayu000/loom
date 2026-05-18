use std::fs;
use std::path::PathBuf;

use serde_json::json;
use uuid::Uuid;

use crate::cli::ImportObservedArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;

use super::super::helpers::{
    commit_registry_state, copy_dir_recursive_without_symlinks, map_git, map_io, map_lock,
    map_registry_state, maybe_autosync_or_queue, record_registry_operation,
    restore_registry_audit_state, rollback_added_skill, snapshot_registry_audit_state,
    validate_skill_name,
};
use super::super::{App, CommandFailure};
use super::shared::{
    observed_import_targets, observed_skill_copy_source, has_skill_entrypoint,
    reset_command_created_commits, unstage_registry_state,
};

impl App {
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
                if let Err(err) = gitops::stage_path(&self.ctx, std::path::Path::new(&skill_rel)) {
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
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let mut changed = false;
        for skill_rel in &imported_rels {
            match gitops::has_staged_changes_for_path(&self.ctx, std::path::Path::new(skill_rel)) {
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
}

pub(super) fn rollback_imported_skills(ctx: &crate::state::AppContext, skill_rels: &[String]) {
    for skill_rel in skill_rels {
        let dst = ctx.root.join(skill_rel);
        rollback_added_skill(ctx, skill_rel, &dst);
    }
}

fn rollback_import_after_commit(
    ctx: &crate::state::AppContext,
    paths: &crate::state_model::RegistryStatePaths,
    registry_backup: &super::super::helpers::RegistryAuditStateBackup,
    previous_head: &str,
    imported_rels: &[String],
) {
    reset_command_created_commits(ctx, previous_head);
    rollback_imported_skills(ctx, imported_rels);
    let _ = restore_registry_audit_state(paths, registry_backup);
    unstage_registry_state(ctx);
}
