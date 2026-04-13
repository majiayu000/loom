use serde_json::json;

use crate::cli::{HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand, SyncCommand};
use crate::envelope::Meta;
use crate::gitops;
use crate::types::ErrorCode;

use super::helpers::{
    map_git, map_io, map_lock, map_replay_conflict, remote_status_payload,
    sync_push_internal, sync_replay_internal,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_sync(
        &self,
        command: &SyncCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            SyncCommand::Status => {
                let (remote, meta) = remote_status_payload(&self.ctx)?;
                Ok((json!({"remote": remote}), meta))
            }
            SyncCommand::Push => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let res = sync_push_internal(&self.ctx)?;
                Ok((json!({"result": res}), Meta::default()))
            }
            SyncCommand::Pull => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                if !gitops::remote_exists(&self.ctx) {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        "remote origin not configured",
                    ));
                }
                if !gitops::fetch_origin_main_if_present(&self.ctx)
                    .map_err(super::helpers::map_remote_unreachable)?
                {
                    return Ok((
                        json!({"result": "remote_empty", "replay": "no_pending_ops"}),
                        Meta::default(),
                    ));
                }
                let history_fetch = gitops::fetch_origin_history_branch_if_present(&self.ctx);
                gitops::pull_rebase_main(&self.ctx).map_err(map_replay_conflict)?;
                let replay = sync_replay_internal(&self.ctx)?;
                let mut meta = Meta::default();
                match history_fetch {
                    Ok(true) => {
                        if let Some(warning) =
                            gitops::sync_history_branch_from_remote(&self.ctx).map_err(map_git)?
                        {
                            meta.warnings.push(warning);
                        }
                    }
                    Ok(false) => {}
                    Err(err) => meta.warnings.push(format!(
                        "failed to fetch origin/{}: {}",
                        gitops::HISTORY_BRANCH,
                        err
                    )),
                }
                Ok((json!({"result": "pulled", "replay": replay}), meta))
            }
            SyncCommand::Replay => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let replay = sync_replay_internal(&self.ctx)?;
                Ok((json!({"result": replay}), Meta::default()))
            }
        }
    }

    pub fn cmd_ops(
        &self,
        command: &OpsCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            OpsCommand::List => {
                let report = self.ctx.read_pending_report().map_err(map_io)?;
                Ok((
                    json!({
                        "count": report.ops.len(),
                        "ops": report.ops,
                        "journal_events": report.journal_events,
                        "history_events": report.history_events
                    }),
                    Meta {
                        warnings: report.warnings,
                        sync_state: None,
                        op_id: None,
                    },
                ))
            }
            OpsCommand::Retry => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let pending_before = self.ctx.pending_count().map_err(map_io)?;
                let result = sync_replay_internal(&self.ctx)?;
                let pending_after = self.ctx.pending_count().map_err(map_io)?;
                Ok((
                    json!({
                        "result": result,
                        "pending_before": pending_before,
                        "pending_after": pending_after
                    }),
                    Meta::default(),
                ))
            }
            OpsCommand::Purge => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_layout()?;
                let purged = self.ctx.purge_pending().map_err(map_io)?;
                Ok((json!({"purged": purged}), Meta::default()))
            }
            OpsCommand::History { command } => self.cmd_ops_history(command),
        }
    }

    fn cmd_ops_history(
        &self,
        command: &OpsHistoryCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            OpsHistoryCommand::Diagnose => {
                let report = gitops::history_status(&self.ctx).map_err(map_git)?;
                Ok((json!(report), Meta::default()))
            }
            OpsHistoryCommand::Repair(args) => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let strategy = match args.strategy {
                    HistoryRepairStrategyArg::Local => gitops::HistoryRepairStrategy::Local,
                    HistoryRepairStrategyArg::Remote => gitops::HistoryRepairStrategy::Remote,
                };
                let report = gitops::repair_history_branch(&self.ctx, strategy).map_err(map_git)?;
                Ok((json!(report), Meta::default()))
            }
        }
    }
}
