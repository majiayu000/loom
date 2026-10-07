use std::thread;
use std::time::Duration;

use crate::cli::WatchArgs;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::{WatchPlan, collect_watch_plan};

#[path = "watch_snapshot.rs"]
mod watch_snapshot;

use watch_snapshot::{WatchPathSnapshot, snapshot_paths};

#[derive(Debug, Eq, PartialEq)]
pub(super) struct WatchSnapshot {
    pub(super) plan: WatchPlan,
    entries: Vec<WatchPathSnapshot>,
}

fn collect_watch_snapshot(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchSnapshot, CommandFailure> {
    let plan = collect_watch_plan(ctx, args)?;
    let path_count = plan.path_count();
    if path_count > args.max_batch {
        return Err(CommandFailure::new(
            ErrorCode::DependencyConflict,
            format!(
                "watch batch has {} changed paths, exceeding --max-batch {}; run manual skill save",
                path_count, args.max_batch
            ),
        ));
    }
    let entries = snapshot_paths(ctx, &plan)?;
    Ok(WatchSnapshot { plan, entries })
}

pub(super) fn collect_stable_watch_plan(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchSnapshot, CommandFailure> {
    collect_stable_watch_plan_with_wait(ctx, args, thread::sleep)
}

fn collect_stable_watch_plan_with_wait(
    ctx: &AppContext,
    args: &WatchArgs,
    mut wait: impl FnMut(Duration),
) -> std::result::Result<WatchSnapshot, CommandFailure> {
    let first = collect_watch_snapshot(ctx, args)?;
    if first.plan.is_empty() || args.debounce_ms == 0 {
        return Ok(first);
    }

    wait(Duration::from_millis(args.debounce_ms));
    let second = collect_watch_snapshot(ctx, args)?;
    if first == second {
        return Ok(second);
    }

    wait(Duration::from_millis(args.debounce_ms));
    let third = collect_watch_snapshot(ctx, args)?;
    if second == third {
        return Ok(third);
    }

    Err(CommandFailure::new(
        ErrorCode::CaptureConflict,
        "skill files changed during autosave debounce; retry after edits settle",
    ))
}

pub(super) fn ensure_watch_snapshot_unchanged(
    ctx: &AppContext,
    args: &WatchArgs,
    expected: &WatchSnapshot,
) -> std::result::Result<(), CommandFailure> {
    if collect_watch_snapshot(ctx, args)? != *expected {
        return Err(CommandFailure::new(
            ErrorCode::CaptureConflict,
            "skill files changed after autosave debounce; retry after edits settle",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "watch_stability_tests.rs"]
mod tests;
