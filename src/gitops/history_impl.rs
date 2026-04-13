use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Result, anyhow};

use crate::state::{
    AppContext, summarize_history_body, synthesize_snapshot_raw_from_segment_bodies,
};

use super::{
    EMPTY_TREE_SHA, HISTORY_ARCHIVES_DIR, HISTORY_BRANCH_REF, HISTORY_COMPACT_AFTER_SEGMENTS,
    HISTORY_RETAIN_ARCHIVES, HISTORY_RETAIN_RECENT_SEGMENTS, HISTORY_SEGMENTS_DIR,
    HISTORY_SNAPSHOT_FILE, TempFile, ensure_local_identity, hash_object_bytes, read_blob, run_git,
    run_git_allow_failure, run_git_in_with_env,
};

use super::history::{
    HistoryConflictReport, HistoryRepairReport, HistoryRepairStrategy, history_status,
};

#[derive(Debug, Clone)]
pub(super) struct HistoryBranchState {
    pub(super) commit: String,
    pub(super) tree: String,
    pub(super) archives: BTreeMap<String, String>,
    pub(super) segments: BTreeMap<String, String>,
    pub(super) snapshot_blob: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct HistoryTreeEntry {
    pub(super) path: String,
    pub(super) blob: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct HistoryRetentionSummary {
    pub(super) compacted_segments: usize,
    pub(super) rolled_archives: usize,
}

impl HistoryRetentionSummary {
    pub(super) fn changed(&self) -> bool {
        self.compacted_segments > 0 || self.rolled_archives > 0
    }
}

#[derive(Debug, Clone)]
pub(super) struct ComposedHistoryState {
    pub(super) archives: BTreeMap<String, String>,
    pub(super) segments: BTreeMap<String, String>,
    pub(super) snapshot_blob: String,
    pub(super) retention: HistoryRetentionSummary,
}

#[derive(Debug)]
struct HistoryBlobEntry {
    path: String,
    body: String,
    last_at: i64,
}

pub(super) fn load_history_branch_state(
    ctx: &AppContext,
    reference: &str,
) -> Result<Option<HistoryBranchState>> {
    let output = run_git_allow_failure(ctx, &["rev-parse", "--verify", "--quiet", reference])?;
    if !output.status.success() {
        return Ok(None);
    }

    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let tree_ref = format!("{}^{{tree}}", reference);
    let tree = run_git(ctx, &["rev-parse", &tree_ref])?;
    let listing = run_git(ctx, &["ls-tree", "-r", reference])?;

    let mut archives = BTreeMap::new();
    let mut segments = BTreeMap::new();
    let mut snapshot_blob = None;
    for line in listing.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (meta, path) = line
            .split_once('\t')
            .ok_or_else(|| anyhow!("unexpected ls-tree output: {}", line))?;
        let mut meta_parts = meta.split_whitespace();
        let _mode = meta_parts
            .next()
            .ok_or_else(|| anyhow!("missing ls-tree mode: {}", line))?;
        let kind = meta_parts
            .next()
            .ok_or_else(|| anyhow!("missing ls-tree kind: {}", line))?;
        let blob = meta_parts
            .next()
            .ok_or_else(|| anyhow!("missing ls-tree object id: {}", line))?;
        if kind != "blob" {
            continue;
        }
        if path == HISTORY_SNAPSHOT_FILE {
            snapshot_blob = Some(blob.to_string());
        } else if path.starts_with(&format!("{}/", HISTORY_ARCHIVES_DIR)) {
            archives.insert(path.to_string(), blob.to_string());
        } else if path.starts_with(&format!("{}/", HISTORY_SEGMENTS_DIR)) {
            segments.insert(path.to_string(), blob.to_string());
        }
    }

    Ok(Some(HistoryBranchState {
        commit,
        tree,
        archives,
        segments,
        snapshot_blob,
    }))
}

pub(super) fn compose_history_state(
    ctx: &AppContext,
    local: &HistoryBranchState,
    remote: Option<&HistoryBranchState>,
    strategy: Option<HistoryRepairStrategy>,
) -> Result<ComposedHistoryState> {
    let mut archives = local.archives.clone();
    let mut segments = local.segments.clone();
    let mut history_changed = false;

    if let Some(remote) = remote {
        history_changed |= merge_history_map(&mut archives, &remote.archives, "archive", strategy)?;
        history_changed |= merge_history_map(&mut segments, &remote.segments, "segment", strategy)?;
    }

    let retention = apply_history_retention(ctx, &mut archives, &mut segments)?;
    history_changed |= retention.changed();

    let snapshot_blob = if history_changed {
        synthesize_history_snapshot_blob(ctx, &archives, &segments)?
    } else {
        local
            .snapshot_blob
            .clone()
            .or_else(|| remote.and_then(|state| state.snapshot_blob.clone()))
            .unwrap_or(synthesize_history_snapshot_blob(ctx, &archives, &segments)?)
    };

    Ok(ComposedHistoryState {
        archives,
        segments,
        snapshot_blob,
        retention,
    })
}

fn merge_history_map(
    local_map: &mut BTreeMap<String, String>,
    remote_map: &BTreeMap<String, String>,
    scope: &str,
    strategy: Option<HistoryRepairStrategy>,
) -> Result<bool> {
    let mut changed = false;
    let mut used_paths = local_map.keys().cloned().collect::<BTreeSet<_>>();

    for (path, remote_blob) in remote_map {
        match local_map.get(path) {
            None => {
                local_map.insert(path.clone(), remote_blob.clone());
                used_paths.insert(path.clone());
                changed = true;
            }
            Some(local_blob) if local_blob == remote_blob => {}
            Some(local_blob) => {
                let strategy = strategy.ok_or_else(|| {
                    anyhow!(
                        "loom-history {} path conflict at {}; run `loom ops history repair --strategy <local|remote>`",
                        scope,
                        path
                    )
                })?;

                match strategy {
                    HistoryRepairStrategy::Local => {
                        let renamed = unique_repair_path(&used_paths, path, "remote", remote_blob);
                        if !local_map.contains_key(&renamed) {
                            local_map.insert(renamed.clone(), remote_blob.clone());
                            used_paths.insert(renamed);
                        }
                        changed = true;
                    }
                    HistoryRepairStrategy::Remote => {
                        let renamed = unique_repair_path(&used_paths, path, "local", local_blob);
                        if !local_map.contains_key(&renamed) {
                            local_map.insert(renamed.clone(), local_blob.clone());
                            used_paths.insert(renamed);
                        }
                        local_map.insert(path.clone(), remote_blob.clone());
                        changed = true;
                    }
                }
            }
        }
    }

    Ok(changed)
}

pub(super) fn detect_history_conflicts(
    local: &HistoryBranchState,
    remote: &HistoryBranchState,
) -> Vec<HistoryConflictReport> {
    let mut reports = detect_history_map_conflicts(&local.archives, &remote.archives, "archive");
    reports.extend(detect_history_map_conflicts(
        &local.segments,
        &remote.segments,
        "segment",
    ));
    reports
}

fn detect_history_map_conflicts(
    local_map: &BTreeMap<String, String>,
    remote_map: &BTreeMap<String, String>,
    scope: &str,
) -> Vec<HistoryConflictReport> {
    let used_paths = local_map
        .keys()
        .chain(remote_map.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    local_map
        .iter()
        .filter_map(|(path, local_blob)| {
            let remote_blob = remote_map.get(path)?;
            if local_blob == remote_blob {
                return None;
            }

            Some(HistoryConflictReport {
                scope: scope.to_string(),
                path: path.clone(),
                local_blob: short_object_id(local_blob),
                remote_blob: short_object_id(remote_blob),
                local_rename_path: unique_repair_path(&used_paths, path, "local", local_blob),
                remote_rename_path: unique_repair_path(&used_paths, path, "remote", remote_blob),
            })
        })
        .collect()
}

pub(super) fn apply_history_retention(
    ctx: &AppContext,
    archives: &mut BTreeMap<String, String>,
    segments: &mut BTreeMap<String, String>,
) -> Result<HistoryRetentionSummary> {
    let mut summary = HistoryRetentionSummary::default();

    if segments.len() > HISTORY_COMPACT_AFTER_SEGMENTS {
        let mut entries = history_blob_entries(ctx, segments)?;
        entries.sort_by(|left, right| {
            left.last_at
                .cmp(&right.last_at)
                .then_with(|| left.path.cmp(&right.path))
        });
        let compact_count = entries.len().saturating_sub(HISTORY_RETAIN_RECENT_SEGMENTS);
        if compact_count >= 2 {
            let retired = entries.into_iter().take(compact_count).collect::<Vec<_>>();
            let retired_paths = retired
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            let archive_body = join_history_bodies(retired.iter().map(|entry| entry.body.as_str()));
            let archive_blob = hash_object_bytes(ctx, archive_body.as_bytes())?;
            let existing_paths = archives.keys().cloned().collect::<BTreeSet<_>>();
            let archive_path = unique_archive_path(&existing_paths, &retired_paths, &archive_blob);
            for path in &retired_paths {
                segments.remove(path);
            }
            archives.insert(archive_path, archive_blob);
            summary.compacted_segments = retired_paths.len();
        }
    }

    if archives.len() > HISTORY_RETAIN_ARCHIVES {
        let mut entries = history_blob_entries(ctx, archives)?;
        entries.sort_by(|left, right| {
            left.last_at
                .cmp(&right.last_at)
                .then_with(|| left.path.cmp(&right.path))
        });
        let roll_count = entries.len().saturating_sub(HISTORY_RETAIN_ARCHIVES) + 1;
        if roll_count >= 2 {
            let retired = entries.into_iter().take(roll_count).collect::<Vec<_>>();
            let retired_paths = retired
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            let archive_body = join_history_bodies(retired.iter().map(|entry| entry.body.as_str()));
            let archive_blob = hash_object_bytes(ctx, archive_body.as_bytes())?;
            let existing_paths = archives
                .keys()
                .filter(|path| !retired_paths.iter().any(|retired| retired == *path))
                .cloned()
                .collect::<BTreeSet<_>>();
            let archive_path = unique_archive_path(&existing_paths, &retired_paths, &archive_blob);
            for path in &retired_paths {
                archives.remove(path);
            }
            archives.insert(archive_path, archive_blob);
            summary.rolled_archives = retired_paths.len();
        }
    }

    Ok(summary)
}

pub(super) fn synthesize_history_snapshot_blob(
    ctx: &AppContext,
    archives: &BTreeMap<String, String>,
    segments: &BTreeMap<String, String>,
) -> Result<String> {
    let bodies = history_bodies(ctx, archives, segments)?;
    let snapshot_raw = synthesize_snapshot_raw_from_segment_bodies(&bodies)?;
    hash_object_bytes(ctx, format!("{}\n", snapshot_raw).as_bytes())
}

fn history_bodies(
    ctx: &AppContext,
    archives: &BTreeMap<String, String>,
    segments: &BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let mut bodies = Vec::with_capacity(archives.len() + segments.len());
    for blob in archives.values() {
        bodies.push(read_blob(ctx, blob)?);
    }
    for blob in segments.values() {
        bodies.push(read_blob(ctx, blob)?);
    }
    Ok(bodies)
}

fn history_blob_entries(
    ctx: &AppContext,
    blobs: &BTreeMap<String, String>,
) -> Result<Vec<HistoryBlobEntry>> {
    let mut entries = Vec::with_capacity(blobs.len());
    for (path, blob) in blobs {
        let body = read_blob(ctx, blob)?;
        let summary = summarize_history_body(&body)?;
        let last_at = summary
            .last_at
            .or(summary.first_at)
            .map(|at| at.timestamp_micros())
            .unwrap_or_default();
        entries.push(HistoryBlobEntry {
            path: path.clone(),
            body,
            last_at,
        });
    }
    Ok(entries)
}

pub(super) fn history_tree_entries(state: &ComposedHistoryState) -> Vec<HistoryTreeEntry> {
    let mut entries = Vec::with_capacity(state.archives.len() + state.segments.len() + 1);
    for (path, blob) in &state.archives {
        entries.push(HistoryTreeEntry {
            path: path.clone(),
            blob: blob.clone(),
        });
    }
    for (path, blob) in &state.segments {
        entries.push(HistoryTreeEntry {
            path: path.clone(),
            blob: blob.clone(),
        });
    }
    entries.push(HistoryTreeEntry {
        path: HISTORY_SNAPSHOT_FILE.to_string(),
        blob: state.snapshot_blob.clone(),
    });
    entries
}

pub(super) fn build_tree_from_entries(
    ctx: &AppContext,
    entries: &[HistoryTreeEntry],
) -> Result<String> {
    let index = TempFile::new("loom-history-index")?;
    let index_path = index.path.to_string_lossy().to_string();
    let envs = [("GIT_INDEX_FILE", index_path.as_str())];

    run_git_in_with_env(&ctx.root, &envs, &["read-tree", EMPTY_TREE_SHA])?;
    for entry in entries {
        run_git_in_with_env(
            &ctx.root,
            &envs,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "100644",
                &entry.blob,
                &entry.path,
            ],
        )?;
    }
    run_git_in_with_env(&ctx.root, &envs, &["write-tree"])
}

pub(super) fn create_commit_tree(
    ctx: &AppContext,
    tree: &str,
    parents: &[&str],
    message: &str,
) -> Result<String> {
    ensure_local_identity(ctx)?;

    let mut args = vec!["commit-tree", tree];
    for parent in parents {
        args.push("-p");
        args.push(parent);
    }
    args.push("-m");
    args.push(message);
    run_git(ctx, &args)
}

pub(super) fn update_ref(
    ctx: &AppContext,
    ref_name: &str,
    new_value: &str,
    old_value: Option<&str>,
) -> Result<()> {
    let mut args = vec!["update-ref", ref_name, new_value];
    if let Some(old_value) = old_value {
        args.push(old_value);
    }
    run_git(ctx, &args)?;
    Ok(())
}

pub(super) fn compact_local_history_branch(
    ctx: &AppContext,
    local: &HistoryBranchState,
    strategy: HistoryRepairStrategy,
) -> Result<HistoryRepairReport> {
    let composed = compose_history_state(ctx, local, None, None)?;
    let new_tree = build_tree_from_entries(ctx, &history_tree_entries(&composed))?;
    if !composed.retention.changed() || new_tree == local.tree {
        let status = history_status(ctx)?;
        return Ok(HistoryRepairReport {
            result: "noop".to_string(),
            strategy: strategy.as_str().to_string(),
            commit: None,
            repaired_conflicts: 0,
            compacted_segments: 0,
            rolled_archives: 0,
            local_segments: status.local_segments,
            local_archives: status.local_archives,
            local_snapshot: status.local_snapshot,
            conflicts: Vec::new(),
        });
    }

    let commit = create_commit_tree(
        ctx,
        &new_tree,
        &[local.commit.as_str()],
        "Compact loom-history retention",
    )?;
    update_ref(ctx, HISTORY_BRANCH_REF, &commit, Some(&local.commit))?;
    let status = history_status(ctx)?;

    Ok(HistoryRepairReport {
        result: "compacted".to_string(),
        strategy: strategy.as_str().to_string(),
        commit: Some(commit),
        repaired_conflicts: 0,
        compacted_segments: composed.retention.compacted_segments,
        rolled_archives: composed.retention.rolled_archives,
        local_segments: status.local_segments,
        local_archives: status.local_archives,
        local_snapshot: status.local_snapshot,
        conflicts: Vec::new(),
    })
}

fn join_history_bodies<'a>(bodies: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for body in bodies {
        out.push_str(body.trim_end_matches('\n'));
        out.push('\n');
    }
    out
}

fn unique_archive_path(
    existing_paths: &BTreeSet<String>,
    source_paths: &[String],
    blob: &str,
) -> String {
    let first = source_paths
        .first()
        .and_then(|path| Path::new(path).file_stem())
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "archive".to_string());
    let last = source_paths
        .last()
        .and_then(|path| Path::new(path).file_stem())
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "archive".to_string());
    let prefix = short_object_id(blob);

    for attempt in 0.. {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{}", attempt)
        };
        let candidate = format!(
            "{}/{:03}-{}-{}-{}{}.jsonl",
            HISTORY_ARCHIVES_DIR,
            source_paths.len(),
            sanitize_path_token(&first),
            sanitize_path_token(&last),
            prefix,
            suffix
        );
        if !existing_paths.contains(&candidate) {
            return candidate;
        }
    }

    unreachable!()
}

fn unique_repair_path(
    existing_paths: &BTreeSet<String>,
    original_path: &str,
    side: &str,
    blob: &str,
) -> String {
    let original = Path::new(original_path);
    let parent = original
        .parent()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = original
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "history".to_string());
    let ext = original
        .extension()
        .map(|value| value.to_string_lossy().to_string());
    let prefix = short_object_id(blob);

    for attempt in 0.. {
        let suffix = if attempt == 0 {
            format!("__{}_{}", side, prefix)
        } else {
            format!("__{}_{}_{}", side, prefix, attempt)
        };
        let file = match &ext {
            Some(ext) if !ext.is_empty() => format!("{}{}.{}", stem, suffix, ext),
            _ => format!("{}{}", stem, suffix),
        };
        let candidate = if parent.is_empty() {
            file
        } else {
            format!("{}/{}", parent, file)
        };
        if !existing_paths.contains(&candidate) {
            return candidate;
        }
    }

    unreachable!()
}

fn short_object_id(value: &str) -> String {
    value.chars().take(12).collect()
}

fn sanitize_path_token(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .take(24)
        .collect()
}
