use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde_json::json;

use crate::state::AppContext;
use crate::types::PendingOp;

use super::{
    HISTORY_BRANCH, HISTORY_BRANCH_REF, HISTORY_COMPACT_AFTER_SEGMENTS, HISTORY_RETAIN_ARCHIVES,
    HISTORY_RETAIN_RECENT_SEGMENTS, ORIGIN_HISTORY_BRANCH_REF, ahead_behind_refs,
    ensure_local_identity, hash_object_bytes, hash_object_file, read_blob, remote_exists,
    remote_tracking_history_exists, repo_is_initialized,
};

use super::history_impl::{
    ComposedHistoryState, apply_history_retention, build_tree_from_entries,
    compact_local_history_branch, compose_history_state, create_commit_tree,
    detect_history_conflicts, history_tree_entries, load_history_branch_state,
    synthesize_history_snapshot_blob, update_ref,
};
use super::history_types::{HistoryRepairReport, HistoryRepairStrategy, HistoryStatusReport};

pub fn history_status(ctx: &AppContext) -> Result<HistoryStatusReport> {
    if !repo_is_initialized(ctx)? {
        return Ok(HistoryStatusReport {
            compact_after_segments: HISTORY_COMPACT_AFTER_SEGMENTS,
            retain_recent_segments: HISTORY_RETAIN_RECENT_SEGMENTS,
            retain_archives: HISTORY_RETAIN_ARCHIVES,
            ..HistoryStatusReport::default()
        });
    }

    let local = load_history_branch_state(ctx, HISTORY_BRANCH_REF)?;
    let remote = if remote_tracking_history_exists(ctx)? {
        load_history_branch_state(ctx, ORIGIN_HISTORY_BRANCH_REF)?
    } else {
        None
    };
    let (ahead, behind) = match (&local, &remote) {
        (Some(_), Some(_)) => ahead_behind_refs(ctx, "origin/loom-history", HISTORY_BRANCH)?,
        _ => (0, 0),
    };
    let conflicts = match (&local, &remote) {
        (Some(local), Some(remote)) => detect_history_conflicts(local, remote),
        _ => Vec::new(),
    };

    Ok(HistoryStatusReport {
        local_branch: local.is_some(),
        remote_tracking: remote.is_some(),
        ahead,
        behind,
        local_segments: local.as_ref().map_or(0, |state| state.segments.len()),
        local_archives: local.as_ref().map_or(0, |state| state.archives.len()),
        remote_segments: remote.as_ref().map_or(0, |state| state.segments.len()),
        remote_archives: remote.as_ref().map_or(0, |state| state.archives.len()),
        local_snapshot: local
            .as_ref()
            .is_some_and(|state| state.snapshot_blob.is_some()),
        remote_snapshot: remote
            .as_ref()
            .is_some_and(|state| state.snapshot_blob.is_some()),
        compact_after_segments: HISTORY_COMPACT_AFTER_SEGMENTS,
        retain_recent_segments: HISTORY_RETAIN_RECENT_SEGMENTS,
        retain_archives: HISTORY_RETAIN_ARCHIVES,
        conflicts,
    })
}

pub fn history_journal_bodies(ctx: &AppContext) -> Result<Vec<(String, String)>> {
    if !repo_is_initialized(ctx)? {
        return Ok(Vec::new());
    }
    let Some(local) = load_history_branch_state(ctx, HISTORY_BRANCH_REF)? else {
        return Ok(Vec::new());
    };

    let mut bodies = Vec::with_capacity(local.archives.len() + local.segments.len());
    for (path, blob) in local.archives.iter().chain(local.segments.iter()) {
        bodies.push((path.clone(), read_blob(ctx, blob)?));
    }
    Ok(bodies)
}

pub fn append_history_audit_event(
    ctx: &AppContext,
    command: &str,
    details: serde_json::Value,
    request_id: &str,
) -> Result<String> {
    ensure_local_identity(ctx)?;

    let op = PendingOp::new(command, details, request_id.to_string());
    let op_id = op.stable_id();
    let event_id = uuid::Uuid::new_v4().to_string();
    let at = Utc::now();
    let raw = serde_json::to_string(&json!({
        "event": "audited",
        "event_id": event_id,
        "at": at,
        "op": op,
    }))? + "\n";
    let segment_blob = hash_object_bytes(ctx, raw.as_bytes())?;
    let segment_ref_path = format!(
        "pending_ops_history/00001-{}.jsonl",
        event_id.replace('-', "")
    );

    let base = load_history_branch_state(ctx, HISTORY_BRANCH_REF)?;
    let mut archives = base
        .as_ref()
        .map_or_else(BTreeMap::new, |state| state.archives.clone());
    let mut segments = base
        .as_ref()
        .map_or_else(BTreeMap::new, |state| state.segments.clone());
    segments.insert(segment_ref_path, segment_blob);

    let retention = apply_history_retention(ctx, &mut archives, &mut segments)?;
    let snapshot_blob = synthesize_history_snapshot_blob(ctx, &archives, &segments)?;
    let composed = ComposedHistoryState {
        archives,
        segments,
        snapshot_blob,
        retention,
    };
    let new_tree = build_tree_from_entries(ctx, &history_tree_entries(&composed))?;
    let parents = base
        .as_ref()
        .map(|state| vec![state.commit.as_str()])
        .unwrap_or_default();
    let commit = create_commit_tree(
        ctx,
        &new_tree,
        &parents,
        &format!("Audit operation {}", command),
    )?;
    let expected_old = base.as_ref().map(|state| state.commit.as_str());
    update_ref(ctx, HISTORY_BRANCH_REF, &commit, expected_old)?;

    Ok(op_id)
}

pub fn repair_history_branch(
    ctx: &AppContext,
    strategy: HistoryRepairStrategy,
) -> Result<HistoryRepairReport> {
    if !repo_is_initialized(ctx)? {
        return Ok(HistoryRepairReport::noop(strategy));
    }

    let remote_history_exists = if remote_exists(ctx) {
        super::fetch_origin_history_branch_if_present(ctx)?
    } else {
        false
    };
    let local = load_history_branch_state(ctx, HISTORY_BRANCH_REF)?;
    let remote = if remote_history_exists {
        load_history_branch_state(ctx, ORIGIN_HISTORY_BRANCH_REF)?
    } else {
        None
    };

    match (local, remote) {
        (None, None) => Ok(HistoryRepairReport::noop(strategy)),
        (None, Some(remote)) => {
            update_ref(ctx, HISTORY_BRANCH_REF, &remote.commit, None)?;
            let status = history_status(ctx)?;
            Ok(HistoryRepairReport::from_status(
                "created_from_remote",
                strategy,
                Some(remote.commit),
                &status,
            ))
        }
        (Some(local), None) => compact_local_history_branch(ctx, &local, strategy),
        (Some(local), Some(remote)) => {
            let (ahead, behind) = ahead_behind_refs(ctx, "origin/loom-history", HISTORY_BRANCH)?;
            if ahead == 0 && behind > 0 {
                update_ref(ctx, HISTORY_BRANCH_REF, &remote.commit, Some(&local.commit))?;
                let status = history_status(ctx)?;
                return Ok(HistoryRepairReport::from_status(
                    "fast_forwarded_from_remote",
                    strategy,
                    Some(remote.commit),
                    &status,
                ));
            }
            if ahead == 0 && behind == 0 {
                return compact_local_history_branch(ctx, &local, strategy);
            }
            if ahead > 0 && behind == 0 {
                return compact_local_history_branch(ctx, &local, strategy);
            }

            let conflicts = detect_history_conflicts(&local, &remote);
            let composed = compose_history_state(ctx, &local, Some(&remote), Some(strategy))?;
            let new_tree = build_tree_from_entries(ctx, &history_tree_entries(&composed))?;
            let commit = create_commit_tree(
                ctx,
                &new_tree,
                &[local.commit.as_str(), remote.commit.as_str()],
                "Repair loom-history branch conflicts",
            )?;
            update_ref(ctx, HISTORY_BRANCH_REF, &commit, Some(&local.commit))?;
            let status = history_status(ctx)?;

            let mut report =
                HistoryRepairReport::from_status("repaired", strategy, Some(commit), &status);
            report.repaired_conflicts = conflicts.len();
            report.compacted_segments = composed.retention.compacted_segments;
            report.rolled_archives = composed.retention.rolled_archives;
            report.conflicts = conflicts;
            Ok(report)
        }
    }
}

pub fn sync_history_branch_from_remote(ctx: &AppContext) -> Result<Option<String>> {
    if !remote_tracking_history_exists(ctx)? {
        return Ok(None);
    }

    let remote = load_history_branch_state(ctx, ORIGIN_HISTORY_BRANCH_REF)?
        .ok_or_else(|| anyhow!("origin/loom-history tracking ref missing after fetch"))?;
    let Some(local) = load_history_branch_state(ctx, HISTORY_BRANCH_REF)? else {
        update_ref(ctx, HISTORY_BRANCH_REF, &remote.commit, None)?;
        return Ok(Some(
            "created local loom-history from origin/loom-history".to_string(),
        ));
    };

    let (ahead, behind) = ahead_behind_refs(ctx, "origin/loom-history", HISTORY_BRANCH)?;
    if ahead == 0 && behind == 0 {
        return Ok(None);
    }
    if ahead == 0 && behind > 0 {
        update_ref(ctx, HISTORY_BRANCH_REF, &remote.commit, Some(&local.commit))?;
        return Ok(Some(
            "fast-forwarded local loom-history from origin/loom-history".to_string(),
        ));
    }
    if ahead > 0 && behind == 0 {
        return Ok(None);
    }

    let conflicts = detect_history_conflicts(&local, &remote);
    if !conflicts.is_empty() {
        return Err(anyhow!(
            "loom-history path conflicts detected; run `loom ops history diagnose` and `loom ops history repair --strategy <local|remote>`"
        ));
    }

    let composed = compose_history_state(ctx, &local, Some(&remote), None)?;
    let new_tree = build_tree_from_entries(ctx, &history_tree_entries(&composed))?;
    let commit = create_commit_tree(
        ctx,
        &new_tree,
        &[local.commit.as_str(), remote.commit.as_str()],
        "Reconcile divergent loom-history branches",
    )?;
    update_ref(ctx, HISTORY_BRANCH_REF, &commit, Some(&local.commit))?;
    Ok(Some(
        "reconciled divergent loom-history branches".to_string(),
    ))
}

pub fn mirror_history_segment(
    ctx: &AppContext,
    segment_path: &Path,
    snapshot_path: &Path,
) -> Result<()> {
    if !repo_is_initialized(ctx)? {
        return Ok(());
    }
    ensure_local_identity(ctx)?;

    let segment_name = segment_path
        .file_name()
        .ok_or_else(|| anyhow!("history segment missing file name"))?
        .to_string_lossy()
        .to_string();
    let segment_ref_path = format!("{}/{}", super::HISTORY_SEGMENTS_DIR, segment_name);

    let base = load_history_branch_state(ctx, HISTORY_BRANCH_REF)?;
    let mut archives = base
        .as_ref()
        .map_or_else(BTreeMap::new, |state| state.archives.clone());
    let mut segments = base
        .as_ref()
        .map_or_else(BTreeMap::new, |state| state.segments.clone());
    let segment_blob = hash_object_file(ctx, segment_path)?;
    segments.insert(segment_ref_path, segment_blob);

    let retention = apply_history_retention(ctx, &mut archives, &mut segments)?;
    let snapshot_blob = if retention.changed() {
        synthesize_history_snapshot_blob(ctx, &archives, &segments)?
    } else {
        hash_object_file(ctx, snapshot_path)?
    };
    let composed = ComposedHistoryState {
        archives,
        segments,
        snapshot_blob,
        retention,
    };
    let new_tree = build_tree_from_entries(ctx, &history_tree_entries(&composed))?;

    if base.as_ref().is_some_and(|state| state.tree == new_tree) {
        return Ok(());
    }

    let parents = base
        .as_ref()
        .map(|state| vec![state.commit.as_str()])
        .unwrap_or_default();
    let commit = create_commit_tree(
        ctx,
        &new_tree,
        &parents,
        &format!("Archive ops segment {}", segment_name),
    )?;
    let expected_old = base.as_ref().map(|state| state.commit.as_str());
    update_ref(ctx, HISTORY_BRANCH_REF, &commit, expected_old)?;
    Ok(())
}

pub fn mirror_pending_ops_history(ctx: &AppContext) -> Result<()> {
    if !repo_is_initialized(ctx)? {
        return Ok(());
    }

    let entries = match fs::read_dir(&ctx.pending_ops_history_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err).context("failed to read pending ops history dir"),
    };

    let mut segments = Vec::new();
    for entry in entries {
        let entry = entry.context("failed to read pending ops history entry")?;
        if entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", entry.path().display()))?
            .is_file()
        {
            segments.push(entry.path());
        }
    }
    segments.sort();

    for segment_path in segments {
        mirror_history_segment(ctx, &segment_path, &ctx.pending_ops_snapshot_file)
            .context("failed to mirror pending ops history into git")?;
    }

    Ok(())
}
