use serde_json::json;

use crate::cli::{TargetAddArgs, TargetCommand, TargetOwnership, WorkspaceInitArgs};
use crate::envelope::Meta;
use crate::state::home_dir;
use crate::types::ErrorCode;

use super::super::helpers::{
    agent_kind_as_str, commit_registry_state, map_lock, maybe_autosync_or_queue,
};
use super::super::{App, CommandFailure};
use super::shared::{DEFAULT_SCAN_AGENTS, default_skill_dir};

impl App {
    pub fn cmd_workspace_init(
        &self,
        args: &WorkspaceInitArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        // Hold the workspace lock for the entire init, including the scan.
        // lock_workspace is reentrant within the same thread, so cmd_target
        // calls below can acquire it again without deadlock.
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.ensure_registry_layout()?;

        let mut imported: Vec<serde_json::Value> = Vec::new();
        let mut skipped: Vec<serde_json::Value> = Vec::new();

        if args.scan_existing {
            let home = home_dir().ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--scan-existing requires HOME or USERPROFILE to be set",
                )
            })?;
            for agent in DEFAULT_SCAN_AGENTS {
                let path = default_skill_dir(agent, home.as_path());
                let path_str = path.display().to_string();
                let p = path.as_path();
                if !p.exists() {
                    skipped.push(json!({
                        "agent": agent_kind_as_str(agent),
                        "path": path_str,
                        "reason": "does-not-exist",
                    }));
                    continue;
                }
                if !p.is_dir() {
                    skipped.push(json!({
                        "agent": agent_kind_as_str(agent),
                        "path": path_str,
                        "reason": "not-a-directory",
                    }));
                    continue;
                }
                let add_args = TargetAddArgs {
                    agent,
                    path: path_str.clone(),
                    ownership: TargetOwnership::Observed,
                };
                let (value, _meta) = self.cmd_target(&TargetCommand::Add(add_args), request_id)?;
                imported.push(value);
            }
        }

        let commit = commit_registry_state(&self.ctx, "workspace: initialize registry state")?;
        let mut meta = Meta::default();
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "workspace.init",
                request_id,
                json!({"commit": commit, "scanned": args.scan_existing}),
                &mut meta,
            )?;
        }

        Ok((
            json!({
                "initialized": true,
                "scanned": args.scan_existing,
                "imported": imported,
                "skipped": skipped,
                "commit": commit,
            }),
            meta,
        ))
    }
}
