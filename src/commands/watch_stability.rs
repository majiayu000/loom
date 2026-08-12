use std::thread;
use std::time::Duration;

use crate::cli::WatchArgs;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::{WatchPlan, collect_watch_plan};

pub(super) fn collect_stable_watch_plan(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchPlan, CommandFailure> {
    let first = collect_watch_plan(ctx, args)?;
    if first.is_empty() || args.debounce_ms == 0 {
        return Ok(first);
    }

    thread::sleep(Duration::from_millis(args.debounce_ms));
    let second = collect_watch_plan(ctx, args)?;
    if first == second {
        return Ok(second);
    }

    thread::sleep(Duration::from_millis(args.debounce_ms));
    let third = collect_watch_plan(ctx, args)?;
    if second == third {
        return Ok(third);
    }

    Err(CommandFailure::new(
        ErrorCode::CaptureConflict,
        "skill files changed during autosave debounce; retry after edits settle",
    ))
}
