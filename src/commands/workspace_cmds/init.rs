use serde_json::json;

use crate::agent_adapters::load_agent_adapters;
use crate::cli::{TargetOwnership, WorkspaceInitArgs};
use crate::envelope::Meta;
use crate::state::home_dir;
use crate::types::ErrorCode;

use super::super::helpers::{commit_registry_state, map_lock};
use super::super::projections::maybe_autosync_or_queue;
use super::super::{App, CommandFailure};

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
        let adapters = if args.scan_existing {
            home_dir().ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--scan-existing requires HOME or USERPROFILE to be set",
                )
            })?;
            Some(load_agent_adapters(&self.ctx)?)
        } else {
            None
        };
        self.ensure_registry_layout()?;

        let mut imported: Vec<serde_json::Value> = Vec::new();
        let mut skipped: Vec<serde_json::Value> = Vec::new();

        if let Some(adapters) = adapters.as_ref() {
            for adapter in adapters.adapters() {
                if !adapter.capabilities.automatic_discovery {
                    continue;
                }
                for path in &adapter.default_skill_dirs {
                    let path_str = path.display().to_string();
                    let p = path.as_path();
                    if !p.exists() {
                        skipped.push(json!({
                            "agent": adapter.id,
                            "agent_source": adapter.source,
                            "path": path_str,
                            "reason": "does-not-exist",
                        }));
                        continue;
                    }
                    if !p.is_dir() {
                        skipped.push(json!({
                            "agent": adapter.id,
                            "agent_source": adapter.source,
                            "path": path_str,
                            "reason": "not-a-directory",
                        }));
                        continue;
                    }
                    let (value, _meta) = self.cmd_target_add_raw(
                        &adapter.id,
                        &path_str,
                        TargetOwnership::Observed,
                        &adapter.source,
                        request_id,
                    )?;
                    imported.push(value);
                }
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
