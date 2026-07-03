use std::collections::BTreeSet;

use serde_json::json;

use crate::cli::{HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand, SyncCommand};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{
    map_git, map_io, map_lock, map_push_rejected, map_remote_unreachable, map_replay_conflict,
};
use super::projections::remote_status_payload;
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
            SyncCommand::Push(args) if args.dry_run => self.cmd_sync_push_plan(),
            SyncCommand::Push(_) => {
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
                        json!({"result": "remote_empty", "replay": "no_operations"}),
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
                let report = self.ctx.read_registry_ops_report().map_err(map_io)?;
                let history = gitops::history_status(&self.ctx).unwrap_or_default();
                Ok((
                    json!({
                        "count": report.ops.len(),
                        "ops": report.ops,
                        "journal_events": 0,
                        "history_events": history.local_segments + history.local_archives,
                        "state_model": "registry"
                    }),
                    Meta::default(),
                ))
            }
            OpsCommand::Retry => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let queued_before = self
                    .ctx
                    .registry_operation_backlog_count()
                    .map_err(map_io)?;
                let result = sync_replay_internal(&self.ctx)?;
                let queued_after = self
                    .ctx
                    .registry_operation_backlog_count()
                    .map_err(map_io)?;
                Ok((
                    json!({
                        "result": result,
                        "queued_before": queued_before,
                        "queued_after": queued_after
                    }),
                    Meta::default(),
                ))
            }
            OpsCommand::Purge => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_layout()?;
                let purged = self.ctx.purge_registry_ops().map_err(map_io)?;
                let _purge_commit = gitops::commit_paths_if_changed(
                    &self.ctx,
                    &[
                        "state/registry/ops/operations.jsonl",
                        "state/registry/ops/checkpoint.json",
                    ],
                    "ops: purge operation records",
                )
                .map_err(map_git)?;
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
                Ok((
                    json!({
                        "result": report.result,
                        "strategy": report.strategy,
                        "commit": report.commit,
                        "repaired_conflicts": report.repaired_conflicts,
                        "compacted_segments": report.compacted_segments,
                        "rolled_archives": report.rolled_archives,
                        "local_segments": report.local_segments,
                        "local_archives": report.local_archives,
                        "local_snapshot": report.local_snapshot,
                        "conflicts": report.conflicts,
                    }),
                    Meta::default(),
                ))
            }
        }
    }
}

pub(crate) fn sync_push_internal(
    ctx: &AppContext,
) -> std::result::Result<&'static str, CommandFailure> {
    if !gitops::remote_exists(ctx) {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "remote origin not configured",
        ));
    }

    let _state_commit = gitops::commit_paths_if_changed(
        ctx,
        &[".gitignore", ".gitattributes", "state/registry", "state/v3"],
        "sync: commit registry state",
    )
    .map_err(map_git)?;
    let operation_report = ctx.read_registry_ops_report().map_err(map_io)?;
    let queued_ids = operation_report
        .ops
        .iter()
        .map(|op| op.op_id.clone())
        .collect::<BTreeSet<_>>();
    let remote_main_exists =
        gitops::fetch_origin_main_if_present(ctx).map_err(map_remote_unreachable)?;
    let remote_history_exists =
        gitops::fetch_origin_history_branch_if_present(ctx).map_err(map_remote_unreachable)?;
    if remote_history_exists {
        gitops::sync_history_branch_from_remote(ctx).map_err(map_git)?;
    }
    if remote_main_exists {
        let (_ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
        if behind > 0 {
            gitops::pull_rebase_main(ctx).map_err(map_replay_conflict)?;
        }
    }
    if let Err(err) = gitops::push_main_with_tags(ctx).map_err(map_push_rejected) {
        ctx.fail_registry_ops(&queued_ids, err.code.as_str(), &err.message)
            .map_err(map_io)?;
        return Err(err);
    }
    ctx.ack_registry_ops(&queued_ids).map_err(map_io)?;
    let _ack_commit = gitops::commit_paths_if_changed(
        ctx,
        &[
            "state/registry/ops/operations.jsonl",
            "state/registry/ops/checkpoint.json",
        ],
        "sync: acknowledge registry operations",
    )
    .map_err(map_git)?;
    gitops::push_main_with_tags(ctx).map_err(map_push_rejected)?;
    Ok("pushed")
}

pub(crate) fn sync_replay_internal(
    ctx: &AppContext,
) -> std::result::Result<&'static str, CommandFailure> {
    let queued = ctx.registry_operation_backlog_count().map_err(map_io)?;
    if queued == 0 {
        return Ok("no_operations");
    }
    sync_push_internal(ctx)?;
    Ok("replayed")
}
