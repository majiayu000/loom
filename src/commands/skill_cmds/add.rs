use std::fs;
use std::path::Path;

use serde_json::json;
use uuid::Uuid;

use crate::cli::AddArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;

use super::super::helpers::{
    copy_dir_recursive_without_symlinks, map_arg, map_git, map_io, map_lock,
    maybe_autosync_or_queue, rollback_added_skill, validate_skill_name,
};
use super::super::{App, CommandFailure};

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
                crate::types::ErrorCode::ArgInvalid,
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
            gitops::validate_git_url(source).map_err(|err| {
                CommandFailure::new(
                    crate::types::ErrorCode::ArgInvalid,
                    format!("invalid git source '{}': {}", source, err),
                )
            })?;
            let clone = gitops::run_git_allow_failure_restricted(
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
                    crate::types::ErrorCode::ArgInvalid,
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
}
