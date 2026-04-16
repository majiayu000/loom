use std::path::Path;

use serde_json::json;

use crate::cli::{DiffArgs, ReleaseArgs, RollbackArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::V3StatePaths;
use crate::types::ErrorCode;

use super::helpers::{
    ensure_skill_exists, map_arg, map_git, map_lock, maybe_autosync_or_queue, validate_skill_name,
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

        let paths = V3StatePaths::from_app_context(&self.ctx);
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
