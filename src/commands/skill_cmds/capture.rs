use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use uuid::Uuid;

use crate::cli::CaptureArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;

use super::super::helpers::{
    backup_path_if_exists, commit_registry_state, map_git, map_io, map_lock, map_registry_state,
    maybe_autosync_or_queue, record_registry_observation, record_registry_operation,
    resolve_capture_projection, restore_path_from_backup, restore_registry_audit_state,
    snapshot_registry_audit_state, update_projection_after_capture,
    copy_dir_recursive_without_symlinks, ensure_skill_exists,
};
use super::super::{App, CommandFailure};
use super::shared::{maybe_skill_fault, rollback_registry_state};

impl App {
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
                crate::types::ErrorCode::ArgInvalid,
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
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    true,
                    &previous_head,
                    &previous_index,
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
                    &skill_path,
                    source_backup.as_ref(),
                    true,
                    &previous_head,
                    &previous_index,
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
        let registry_audit_backup =
            snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
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
                let _ = restore_registry_audit_state(&paths, &registry_audit_backup);
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    source_replaced,
                    &previous_head,
                    &previous_index,
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
                let _ = restore_registry_audit_state(&paths, &registry_audit_backup);
                rollback_capture_mutation(
                    &self.ctx,
                    &skill_path,
                    source_backup.as_ref(),
                    source_replaced,
                    &previous_head,
                    &previous_index,
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
}

fn rollback_capture_mutation(
    ctx: &crate::state::AppContext,
    skill_path: &Path,
    source_backup: Option<&serde_json::Value>,
    source_replaced: bool,
    previous_head: &str,
    previous_index: &gitops::IndexSnapshot,
    commit_created: bool,
) {
    if commit_created {
        let _ = gitops::run_git_allow_failure(ctx, &["reset", "--soft", previous_head]);
    }

    if source_replaced {
        if let Some(backup) = source_backup {
            let _ = restore_path_from_backup(skill_path, backup);
        } else {
            let _ = remove_path_if_exists(skill_path);
        }
    }

    let _ = gitops::restore_index(ctx, previous_index);
}

fn ensure_capture_source_not_drifted(
    ctx: &crate::state::AppContext,
    projection: &crate::state_model::RegistryProjectionInstance,
    skill_rel: &Path,
) -> std::result::Result<(), CommandFailure> {
    let skill_rel_str = skill_rel.to_string_lossy();
    let committed = git_diff_has_changes(
        ctx,
        &[&projection.last_applied_rev, "HEAD", "--", &skill_rel_str],
    )?;
    let staged = git_diff_has_changes(ctx, &["--cached", "--", &skill_rel_str])?;
    let unstaged = git_diff_has_changes(ctx, &["--", &skill_rel_str])?;

    if !(committed || staged || unstaged) {
        return Ok(());
    }

    let current_rev = gitops::head(ctx).map_err(map_git)?;
    let mut failure = CommandFailure::new(
        crate::types::ErrorCode::CaptureConflict,
        format!(
            "source skill '{}' changed since projection '{}'; save or rollback source changes before capture",
            projection.skill_id, projection.instance_id
        ),
    );
    failure.details = json!({
        "skill_id": projection.skill_id,
        "instance_id": projection.instance_id,
        "source_path": skill_rel.display().to_string(),
        "last_applied_rev": projection.last_applied_rev,
        "current_rev": current_rev,
        "committed": committed,
        "staged": staged,
        "unstaged": unstaged
    });
    Err(failure)
}

fn git_diff_has_changes(
    ctx: &crate::state::AppContext,
    args: &[&str],
) -> std::result::Result<bool, CommandFailure> {
    let mut full_args = vec!["diff", "--quiet"];
    full_args.extend(args.iter().copied());
    let output = gitops::run_git_allow_failure(ctx, &full_args).map_err(map_git)?;
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(map_git(anyhow::anyhow!(
            "git {:?} failed: {}",
            full_args,
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}
