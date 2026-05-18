use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::state::AppContext;

use super::exec::run_git_in_with_env;

/// Captured registry index used to restore staging after a failed mutation.
///
/// Backs up the underlying active Git index file as a whole, so every per-entry
/// flag (`skip-worktree`, `assume-unchanged`, intent-to-add, fsmonitor cache)
/// survives the rollback. The previous `git write-tree`/`read-tree` design
/// dropped these flags because tree objects only encode `path → blob`.
///
/// The backup file lives under `state_dir/index-snapshots/`. `Drop` removes
/// the file best-effort once the snapshot goes out of scope.
pub struct IndexSnapshot {
    backup_path: PathBuf,
}

impl IndexSnapshot {
    /// Path of the on-disk backup. Exposed for diagnostics; callers should
    /// prefer [`restore_index`] for the actual rollback.
    #[allow(dead_code)]
    pub fn backup_path(&self) -> &Path {
        &self.backup_path
    }
}

impl Drop for IndexSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.backup_path);
    }
}

pub fn snapshot_index(ctx: &AppContext) -> Result<IndexSnapshot> {
    snapshot_index_with_env(ctx, &[])
}

pub(super) fn snapshot_index_with_env(
    ctx: &AppContext,
    envs: &[(&str, &str)],
) -> Result<IndexSnapshot> {
    let index_path = resolve_git_index_path(ctx, envs)?;
    if !index_path.exists() {
        return Err(anyhow!(
            "git index missing at {}; cannot snapshot",
            index_path.display()
        ));
    }
    let snapshot_dir = ctx.state_dir.join("index-snapshots");
    fs::create_dir_all(&snapshot_dir)
        .with_context(|| format!("create {}", snapshot_dir.display()))?;
    let backup_path = snapshot_dir.join(format!("snapshot-{}", uuid::Uuid::new_v4()));
    fs::copy(&index_path, &backup_path).with_context(|| {
        format!(
            "back up git index from {} to {}",
            index_path.display(),
            backup_path.display()
        )
    })?;
    Ok(IndexSnapshot { backup_path })
}

pub fn restore_index(ctx: &AppContext, snapshot: &IndexSnapshot) -> Result<()> {
    restore_index_with_env(ctx, snapshot, &[])
}

pub(super) fn restore_index_with_env(
    ctx: &AppContext,
    snapshot: &IndexSnapshot,
    envs: &[(&str, &str)],
) -> Result<()> {
    let index_path = resolve_git_index_path(ctx, envs)?;
    let parent = index_path
        .parent()
        .ok_or_else(|| anyhow!("git index path has no parent: {}", index_path.display()))?;
    // Stage the restore next to the active index so the final rename is
    // intra-directory (atomic on POSIX).
    let staging = parent.join(format!(".loom-index-restore-{}", uuid::Uuid::new_v4()));
    fs::copy(&snapshot.backup_path, &staging).with_context(|| {
        format!(
            "stage index restore at {} from {}",
            staging.display(),
            snapshot.backup_path.display()
        )
    })?;
    crate::fs_util::rename_atomic(&staging, &index_path)
        .with_context(|| format!("install restored index at {}", index_path.display()))?;
    Ok(())
}

fn resolve_git_index_path(ctx: &AppContext, envs: &[(&str, &str)]) -> Result<PathBuf> {
    let raw = run_git_in_with_env(&ctx.root, envs, &["rev-parse", "--git-path", "index"])?;
    let candidate = PathBuf::from(&raw);
    if candidate.is_absolute() {
        Ok(candidate)
    } else {
        Ok(ctx.root.join(&candidate))
    }
}
