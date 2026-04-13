use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use serde::Serialize;

use crate::state::AppContext;

use super::{
    HISTORY_BRANCH, HISTORY_BRANCH_REF, HISTORY_COMPACT_AFTER_SEGMENTS, HISTORY_RETAIN_ARCHIVES,
    HISTORY_RETAIN_RECENT_SEGMENTS, ORIGIN_HISTORY_BRANCH_REF, ahead_behind_refs,
    ensure_local_identity, hash_object_file, remote_exists, remote_tracking_history_exists,
    repo_is_initialized,
};

use super::history_impl::{
    apply_history_retention, build_tree_from_entries, compact_local_history_branch,
    compose_history_state, create_commit_tree, detect_history_conflicts, history_tree_entries,
    load_history_branch_state, synthesize_history_snapshot_blob, update_ref, ComposedHistoryState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRepairStrategy {
    Local,
    Remote,
}

impl HistoryRepairStrategy {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HistoryStatusReport {
    pub local_branch: bool,
    pub remote_tracking: bool,
    pub ahead: u32,
    pub behind: u32,
    pub local_segments: usize,
    pub local_archives: usize,
    pub remote_segments: usize,
    pub remote_archives: usize,
    pub local_snapshot: bool,
    pub remote_snapshot: bool,
    pub compact_after_segments: usize,
    pub retain_recent_segments: usize,
    pub retain_archives: usize,
    pub conflicts: Vec<HistoryConflictReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryConflictReport {
    pub scope: String,
    pub path: String,
    pub local_blob: String,
    pub remote_blob: String,
    pub local_rename_path: String,
    pub remote_rename_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryRepairReport {
    pub result: String,
    pub strategy: String,
    pub commit: Option<String>,
    pub repaired_conflicts: usize,
    pub compacted_segments: usize,
    pub rolled_archives: usize,
    pub local_segments: usize,
    pub local_archives: usize,
    pub local_snapshot: bool,
    pub conflicts: Vec<HistoryConflictReport>,
}

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

pub fn repair_history_branch(
    ctx: &AppContext,
    strategy: HistoryRepairStrategy,
) -> Result<HistoryRepairReport> {
    if !repo_is_initialized(ctx)? {
        return Ok(HistoryRepairReport {
            result: "noop".to_string(),
            strategy: strategy.as_str().to_string(),
            commit: None,
            repaired_conflicts: 0,
            compacted_segments: 0,
            rolled_archives: 0,
            local_segments: 0,
            local_archives: 0,
            local_snapshot: false,
            conflicts: Vec::new(),
        });
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
        (None, None) => Ok(HistoryRepairReport {
            result: "noop".to_string(),
            strategy: strategy.as_str().to_string(),
            commit: None,
            repaired_conflicts: 0,
            compacted_segments: 0,
            rolled_archives: 0,
            local_segments: 0,
            local_archives: 0,
            local_snapshot: false,
            conflicts: Vec::new(),
        }),
        (None, Some(remote)) => {
            update_ref(ctx, HISTORY_BRANCH_REF, &remote.commit, None)?;
            let status = history_status(ctx)?;
            Ok(HistoryRepairReport {
                result: "created_from_remote".to_string(),
                strategy: strategy.as_str().to_string(),
                commit: Some(remote.commit),
                repaired_conflicts: 0,
                compacted_segments: 0,
                rolled_archives: 0,
                local_segments: status.local_segments,
                local_archives: status.local_archives,
                local_snapshot: status.local_snapshot,
                conflicts: Vec::new(),
            })
        }
        (Some(local), None) => compact_local_history_branch(ctx, &local, strategy),
        (Some(local), Some(remote)) => {
            let (ahead, behind) = ahead_behind_refs(ctx, "origin/loom-history", HISTORY_BRANCH)?;
            if ahead == 0 && behind > 0 {
                update_ref(ctx, HISTORY_BRANCH_REF, &remote.commit, Some(&local.commit))?;
                let status = history_status(ctx)?;
                return Ok(HistoryRepairReport {
                    result: "fast_forwarded_from_remote".to_string(),
                    strategy: strategy.as_str().to_string(),
                    commit: Some(remote.commit),
                    repaired_conflicts: 0,
                    compacted_segments: 0,
                    rolled_archives: 0,
                    local_segments: status.local_segments,
                    local_archives: status.local_archives,
                    local_snapshot: status.local_snapshot,
                    conflicts: Vec::new(),
                });
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

            Ok(HistoryRepairReport {
                result: "repaired".to_string(),
                strategy: strategy.as_str().to_string(),
                commit: Some(commit),
                repaired_conflicts: conflicts.len(),
                compacted_segments: composed.retention.compacted_segments,
                rolled_archives: composed.retention.rolled_archives,
                local_segments: status.local_segments,
                local_archives: status.local_archives,
                local_snapshot: status.local_snapshot,
                conflicts,
            })
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
