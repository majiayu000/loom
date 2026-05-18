use chrono::Utc;
use serde_json::json;

use crate::cli::SkillOnlyArgs;
use crate::envelope::Meta;
use crate::gitops;

use super::super::helpers::{
    map_arg, map_git, map_lock, maybe_autosync_or_queue, validate_skill_name, ensure_skill_exists,
};
use super::super::{App, CommandFailure};

impl App {
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
