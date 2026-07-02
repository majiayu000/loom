use super::*;
use crate::cli::ReleaseArgs;

impl App {
    pub fn cmd_release_anchor(
        &self,
        args: &ReleaseArgs,
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
        if let Err(err) = gitops::append_history_audit_event(
            &self.ctx,
            "skill.release",
            json!({"skill": args.skill, "tag": tag.clone(), "anchor": true}),
            request_id,
        ) {
            let mut failure = map_git(err);
            let rollback_output = gitops::run_git_allow_failure(&self.ctx, &["tag", "-d", &tag]);
            let mut rollback_errors = Vec::new();
            match rollback_output {
                Ok(output) if output.status.success() => {}
                Ok(output) => rollback_errors.push(json!({
                    "step": "delete_snapshot_tag",
                    "message": String::from_utf8_lossy(&output.stderr).trim().to_string()
                })),
                Err(err) => rollback_errors.push(json!({
                    "step": "delete_snapshot_tag",
                    "message": err.to_string()
                })),
            }
            if !rollback_errors.is_empty() {
                failure.details = json!({ "rollback_errors": rollback_errors });
            }
            return Err(failure);
        }

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "release",
            request_id,
            json!({"skill": args.skill, "tag": tag, "anchor": true}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "tag": tag, "anchor": true}),
            meta,
        ))
    }
}
