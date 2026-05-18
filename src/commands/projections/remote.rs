use serde_json::json;

use crate::envelope::Meta;
use crate::gitops;
use crate::state::{AppContext, PendingOpsReport};
use crate::types::{ErrorCode, SyncState};

use crate::commands::CommandFailure;
use crate::commands::event_store::redact_sensitive_string;
use crate::commands::helpers::{map_git, map_io, map_push_rejected, map_queue, map_remote_unreachable};

// ---------------------------------------------------------------------------
// Remote status / sync internals
// ---------------------------------------------------------------------------

pub fn remote_status_payload(
    ctx: &AppContext,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let pending_report = ctx.read_pending_report().map_err(map_io)?;
    remote_status_payload_with_pending(ctx, pending_report)
}

pub(crate) fn remote_status_payload_with_pending(
    ctx: &AppContext,
    pending_report: PendingOpsReport,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let pending = pending_report.ops.len();

    if !gitops::remote_exists(ctx) {
        return Ok((
            json!({
                "configured": false,
                "pending_ops": pending,
                "sync_state": SyncState::LocalOnly,
            }),
            Meta {
                warnings: pending_report
                    .warnings
                    .into_iter()
                    .chain(std::iter::once("remote origin not configured".to_string()))
                    .collect(),
                sync_state: Some(SyncState::LocalOnly),
                op_id: None,
            },
        ));
    }

    let url = gitops::remote_url(ctx)
        .map_err(map_git)?
        .unwrap_or_default();
    let redacted_url = redact_sensitive_string(&url);
    let mut meta = Meta {
        warnings: pending_report.warnings,
        sync_state: None,
        op_id: None,
    };

    if !gitops::remote_tracking_main_exists(ctx).map_err(map_git)? {
        let sync_state = if pending > 0 {
            SyncState::PendingPush
        } else {
            SyncState::LocalOnly
        };
        meta.warnings.push(
            "origin/main has not been fetched yet; status is based on local state".to_string(),
        );
        meta.sync_state = Some(sync_state.clone());
        return Ok((
            json!({
                "configured": true,
                "remote": "origin",
                "url": redacted_url,
                "pending_ops": pending,
                "tracking_ref": false,
                "sync_state": sync_state,
            }),
            meta,
        ));
    }

    let (ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
    let sync_state = if pending > 0 {
        SyncState::PendingPush
    } else if ahead == 0 && behind == 0 {
        SyncState::Synced
    } else if ahead > 0 && behind == 0 {
        SyncState::PendingPush
    } else {
        SyncState::Diverged
    };
    meta.sync_state = Some(sync_state.clone());

    Ok((
        json!({
            "configured": true,
            "remote": "origin",
            "url": redacted_url,
            "ahead": ahead,
            "behind": behind,
            "pending_ops": pending,
            "tracking_ref": true,
            "sync_state": sync_state,
        }),
        meta,
    ))
}

pub(crate) fn maybe_autosync_or_queue(
    ctx: &AppContext,
    command: &str,
    request_id: &str,
    details: serde_json::Value,
    meta: &mut Meta,
) -> std::result::Result<(), CommandFailure> {
    if !gitops::remote_exists(ctx) {
        ctx.append_pending(command, details, request_id.to_string())
            .map_err(map_queue)?;
        meta.sync_state = Some(SyncState::PendingPush);
        meta.warnings
            .push("remote origin not configured, operation queued".to_string());
        return Ok(());
    }

    match sync_push_internal(ctx) {
        Ok(_) => {
            meta.sync_state = Some(SyncState::Synced);
        }
        Err(err) => {
            ctx.append_pending(command, details, request_id.to_string())
                .map_err(map_queue)?;
            meta.sync_state = Some(match err.code {
                ErrorCode::RemoteDiverged => SyncState::Diverged,
                ErrorCode::ReplayConflict => SyncState::Conflicted,
                _ => SyncState::PendingPush,
            });
            meta.warnings.push(format!(
                "auto sync failed ({}), operation queued",
                err.code.as_str()
            ));
        }
    }
    Ok(())
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
        &[".gitignore", "state/registry", "state/v3"],
        "sync: commit registry state",
    )
    .map_err(map_git)?;
    let pending_report = ctx.read_pending_report().map_err(map_io)?;
    let queued_ids = pending_report
        .ops
        .iter()
        .map(|op| op.stable_id())
        .collect::<std::collections::BTreeSet<_>>();
    let remote_main_exists =
        gitops::fetch_origin_main_if_present(ctx).map_err(map_remote_unreachable)?;
    let remote_history_exists =
        gitops::fetch_origin_history_branch_if_present(ctx).map_err(map_remote_unreachable)?;
    if remote_history_exists {
        let _ = gitops::sync_history_branch_from_remote(ctx).map_err(map_git)?;
    }
    if remote_main_exists {
        let (_ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
        if behind > 0 {
            return Err(CommandFailure::new(
                ErrorCode::RemoteDiverged,
                "local branch is behind origin/main",
            ));
        }
    }
    gitops::push_main_with_tags(ctx).map_err(map_push_rejected)?;
    ctx.remove_pending_ops(&queued_ids).map_err(map_queue)?;
    Ok("pushed")
}

pub(crate) fn sync_replay_internal(
    ctx: &AppContext,
) -> std::result::Result<&'static str, CommandFailure> {
    let pending = ctx.pending_count().map_err(map_io)?;
    if pending == 0 {
        return Ok("no_pending_ops");
    }
    sync_push_internal(ctx)?;
    Ok("replayed")
}
