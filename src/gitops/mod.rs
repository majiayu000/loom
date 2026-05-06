mod history;
mod history_impl;

pub use history::*;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, anyhow};

use crate::state::AppContext;

pub const HISTORY_BRANCH: &str = "loom-history";
const HISTORY_BRANCH_REF: &str = "refs/heads/loom-history";
const ORIGIN_HISTORY_BRANCH_REF: &str = "refs/remotes/origin/loom-history";
const HISTORY_SEGMENTS_DIR: &str = "pending_ops_history";
const HISTORY_ARCHIVES_DIR: &str = "pending_ops_archive";
const HISTORY_SNAPSHOT_FILE: &str = "pending_ops_snapshot.json";
const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const HISTORY_COMPACT_AFTER_SEGMENTS: usize = 8;
const HISTORY_RETAIN_RECENT_SEGMENTS: usize = 4;
const HISTORY_RETAIN_ARCHIVES: usize = 4;

fn run_git_raw_in_with_env_and_input(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    input: Option<&[u8]>,
    args: &[&str],
) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .current_dir(repo_dir)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    if input.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().context("failed to open git stdin")?;
        stdin
            .write_all(bytes)
            .with_context(|| format!("failed to write git stdin for {:?}", args))?;
    }

    child
        .wait_with_output()
        .with_context(|| format!("failed to read git output for {:?}", args))
}

pub fn run_git(ctx: &AppContext, args: &[&str]) -> Result<String> {
    run_git_in(&ctx.root, args)
}

fn run_git_in(repo_dir: &Path, args: &[&str]) -> Result<String> {
    run_git_in_with_env(repo_dir, &[], args)
}

fn run_git_in_with_env(repo_dir: &Path, envs: &[(&str, &str)], args: &[&str]) -> Result<String> {
    let output = run_git_allow_failure_in_with_env(repo_dir, envs, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {:?} failed: {}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git_in_with_input(repo_dir: &Path, args: &[&str], input: &[u8]) -> Result<String> {
    let output = run_git_raw_in_with_env_and_input(repo_dir, &[], Some(input), args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {:?} failed: {}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_git_allow_failure(ctx: &AppContext, args: &[&str]) -> Result<Output> {
    run_git_allow_failure_in(&ctx.root, args)
}

fn run_git_allow_failure_in(repo_dir: &Path, args: &[&str]) -> Result<Output> {
    run_git_allow_failure_in_with_env(repo_dir, &[], args)
}

fn run_git_allow_failure_in_with_env(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> Result<Output> {
    run_git_raw_in_with_env_and_input(repo_dir, envs, None, args)
}

pub fn ensure_repo_initialized(ctx: &AppContext) -> Result<()> {
    let repo_probe = run_git_allow_failure(ctx, &["rev-parse", "--git-dir"])?;
    if repo_probe.status.success() {
        ensure_local_identity(ctx)?;
        return Ok(());
    }
    if ctx.root.join(".git").exists() {
        return Err(anyhow!("git metadata exists but repository is not healthy"));
    }

    let init_main = run_git_allow_failure(ctx, &["init", "-b", "main"])?;
    if !init_main.status.success() {
        run_git(ctx, &["init"])?;
        let _ = run_git_allow_failure(ctx, &["branch", "-M", "main"])?;
    }

    ensure_local_identity(ctx)?;
    Ok(())
}

pub fn repo_is_initialized(ctx: &AppContext) -> Result<bool> {
    let repo_probe = run_git_allow_failure(ctx, &["rev-parse", "--git-dir"])?;
    Ok(repo_probe.status.success())
}

pub fn has_staged_changes_for_path(ctx: &AppContext, path: &Path) -> Result<bool> {
    let path_str = path.to_string_lossy();
    let output = run_git_allow_failure(ctx, &["diff", "--cached", "--quiet", "--", &path_str])?;
    Ok(!output.status.success())
}

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

fn snapshot_index_with_env(ctx: &AppContext, envs: &[(&str, &str)]) -> Result<IndexSnapshot> {
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

fn restore_index_with_env(
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

pub fn stage_path(ctx: &AppContext, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["add", "--", &path_str])?;
    Ok(())
}

pub fn commit(ctx: &AppContext, message: &str) -> Result<String> {
    run_git(ctx, &["commit", "-m", message])?;
    head(ctx)
}

pub fn head(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["rev-parse", "HEAD"])
}

pub fn short_head(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["rev-parse", "--short", "HEAD"])
}

pub fn create_annotated_tag(ctx: &AppContext, tag: &str, message: &str) -> Result<()> {
    run_git(ctx, &["tag", "-a", tag, "-m", message])?;
    Ok(())
}

pub fn checkout_path_from_ref(ctx: &AppContext, reference: &str, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["checkout", reference, "--", &path_str])?;
    Ok(())
}

pub fn resolve_ref(ctx: &AppContext, reference: &str) -> Result<String> {
    run_git(ctx, &["rev-parse", reference])
}

pub fn set_remote_origin(ctx: &AppContext, url: &str) -> Result<()> {
    let has_origin = run_git_allow_failure(ctx, &["remote", "get-url", "origin"])?;
    if has_origin.status.success() {
        run_git(ctx, &["remote", "set-url", "origin", url])?;
    } else {
        run_git(ctx, &["remote", "add", "origin", url])?;
    }
    Ok(())
}

pub fn remote_exists(ctx: &AppContext) -> bool {
    match run_git_allow_failure(ctx, &["remote", "get-url", "origin"]) {
        Ok(out) => out.status.success(),
        Err(_) => false,
    }
}

pub fn remote_url(ctx: &AppContext) -> Result<Option<String>> {
    let output = run_git_allow_failure(ctx, &["remote", "get-url", "origin"])?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

pub fn fetch_origin_main_if_present(ctx: &AppContext) -> Result<bool> {
    let output = run_git_allow_failure(ctx, &["fetch", "origin", "main"])?;
    if output.status.success() {
        return Ok(true);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.contains("couldn't find remote ref main") {
        return Ok(false);
    }

    Err(anyhow!("git fetch origin main failed: {}", stderr))
}

pub fn fetch_origin_history_branch_if_present(ctx: &AppContext) -> Result<bool> {
    let output = run_git_allow_failure(ctx, &["fetch", "origin", HISTORY_BRANCH])?;
    if output.status.success() {
        return Ok(true);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.contains("couldn't find remote ref") && stderr.contains(HISTORY_BRANCH) {
        return Ok(false);
    }

    Err(anyhow!(
        "git fetch origin {} failed: {}",
        HISTORY_BRANCH,
        stderr
    ))
}

pub fn remote_tracking_main_exists(ctx: &AppContext) -> Result<bool> {
    let output = run_git_allow_failure(
        ctx,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/remotes/origin/main",
        ],
    )?;
    Ok(output.status.success())
}

pub fn remote_tracking_history_exists(ctx: &AppContext) -> Result<bool> {
    let output = run_git_allow_failure(
        ctx,
        &["show-ref", "--verify", "--quiet", ORIGIN_HISTORY_BRANCH_REF],
    )?;
    Ok(output.status.success())
}

pub fn history_branch_exists(ctx: &AppContext) -> Result<bool> {
    let output = run_git_allow_failure(
        ctx,
        &["show-ref", "--verify", "--quiet", HISTORY_BRANCH_REF],
    )?;
    Ok(output.status.success())
}

pub fn ahead_behind_main(ctx: &AppContext) -> Result<(u32, u32)> {
    ahead_behind_refs(ctx, "origin/main", "HEAD")
}

pub fn ahead_behind_refs(ctx: &AppContext, left: &str, right: &str) -> Result<(u32, u32)> {
    let range = format!("{left}...{right}");
    let output = run_git(ctx, &["rev-list", "--left-right", "--count", &range])?;
    let mut parts = output.split_whitespace();
    let left_only = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output"))?
        .parse::<u32>()
        .context("failed to parse left-only count")?;
    let right_only = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output"))?
        .parse::<u32>()
        .context("failed to parse right-only count")?;
    Ok((right_only, left_only))
}

pub fn push_main_with_tags(ctx: &AppContext) -> Result<()> {
    let mut args = vec!["push", "--atomic", "origin", "HEAD:main"];
    if history_branch_exists(ctx)? {
        args.push("loom-history:loom-history");
    }
    args.push("--tags");
    run_git(ctx, &args)?;
    Ok(())
}

pub fn pull_rebase_main(ctx: &AppContext) -> Result<()> {
    let output = run_git_allow_failure(ctx, &["pull", "--rebase", "origin", "main"])?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let _ = run_git_allow_failure(ctx, &["rebase", "--abort"]);

    Err(anyhow!("git pull --rebase origin main failed: {}", stderr))
}

pub fn diff_path(ctx: &AppContext, from: &str, to: &str, path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["diff", from, to, "--", &path_str])
}

pub fn fsck(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["fsck", "--no-progress"])
}

fn hash_object_file(ctx: &AppContext, path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["hash-object", "-w", &path_str])
}

fn hash_object_bytes(ctx: &AppContext, bytes: &[u8]) -> Result<String> {
    run_git_in_with_input(&ctx.root, &["hash-object", "-w", "--stdin"], bytes)
}

fn read_blob(ctx: &AppContext, blob: &str) -> Result<String> {
    run_git(ctx, &["cat-file", "-p", blob])
}

fn ensure_local_identity(ctx: &AppContext) -> Result<()> {
    ensure_local_identity_in(&ctx.root)
}

fn ensure_local_identity_in(repo_dir: &Path) -> Result<()> {
    if !has_local_config_in(repo_dir, "user.name")? {
        run_git_in(repo_dir, &["config", "--local", "user.name", "loom"])?;
    }
    if !has_local_config_in(repo_dir, "user.email")? {
        run_git_in(repo_dir, &["config", "--local", "user.email", "loom@local"])?;
    }
    Ok(())
}

fn has_local_config_in(repo_dir: &Path, key: &str) -> Result<bool> {
    let output = run_git_allow_failure_in(repo_dir, &["config", "--local", "--get", key])?;
    Ok(output.status.success())
}

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(prefix: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::new_v4()));
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppContext;
    use std::process::Command;
    use uuid::Uuid;

    fn fresh_repo(label: &str) -> (AppContext, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("loom-gitops-{}-{}", label, Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        for args in [
            ["init", "-q", "-b", "main"].as_slice(),
            ["config", "user.email", "test@example.com"].as_slice(),
            ["config", "user.name", "test"].as_slice(),
            ["config", "commit.gpgsign", "false"].as_slice(),
            ["config", "tag.gpgSign", "false"].as_slice(),
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("run git");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        // Stage + commit a tracked file so `.git/index` exists and HEAD is real.
        // snapshot_index requires an existing index; flag-bearing tests need a
        // tracked path to attach skip-worktree / assume-unchanged to.
        fs::write(dir.join("base.txt"), "base\n").expect("write base");
        for args in [
            ["add", "base.txt"].as_slice(),
            ["commit", "-q", "-m", "init"].as_slice(),
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("run git");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }

        let ctx = AppContext::new(Some(dir.clone())).expect("build AppContext");
        (ctx, dir)
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        git_ok_with_env(dir, args, &[])
    }

    fn git_ok_with_env(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .envs(envs.iter().copied())
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("git stdout utf8")
    }

    /// `ls-files -v` tag legend: H=cached, h=assume-unchanged, S=skip-worktree,
    /// s=skip-worktree+assume-unchanged.
    fn ls_files_v(dir: &Path) -> String {
        git_ok(dir, &["ls-files", "-v"])
    }

    #[test]
    fn snapshot_round_trip_preserves_intent_to_add() {
        let (ctx, dir) = fresh_repo("ita");

        fs::write(dir.join("ita.txt"), "stand-in").expect("write ita");
        git_ok(&dir, &["add", "-N", "--", "ita.txt"]);
        let before = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            before.lines().any(|l| l == " A ita.txt"),
            "expected IT-A marker pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index(&ctx).expect("snapshot");

        // Clobber the entry the way a stale rollback would.
        git_ok(&dir, &["update-index", "--force-remove", "ita.txt"]);
        let cleared = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            cleared.lines().any(|l| l == "?? ita.txt"),
            "force-remove must turn IT-A into untracked, got:\n{cleared}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let restored = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            restored.lines().any(|l| l == " A ita.txt"),
            "intent-to-add must survive snapshot/restore round trip, got:\n{restored}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_round_trip_respects_alternate_git_index() {
        let (ctx, dir) = fresh_repo("alternate-index");
        let default_index = dir.join(".git").join("index");
        let alternate_index = dir.join("alternate-index");
        fs::copy(&default_index, &alternate_index).expect("seed alternate index");
        let alternate_index = alternate_index.to_string_lossy().to_string();
        let envs = [("GIT_INDEX_FILE", alternate_index.as_str())];

        let default_before = fs::read(&default_index).expect("read default index");
        fs::write(dir.join("alt.txt"), "stand-in").expect("write alternate ita");
        git_ok_with_env(&dir, &["add", "-N", "--", "alt.txt"], &envs);
        let before = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
        assert!(
            before.lines().any(|l| l == " A alt.txt"),
            "expected IT-A marker in alternate index pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index_with_env(&ctx, &envs).expect("snapshot alternate index");

        git_ok_with_env(&dir, &["update-index", "--force-remove", "alt.txt"], &envs);
        let cleared = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
        assert!(
            cleared.lines().any(|l| l == "?? alt.txt"),
            "force-remove must clear alternate index entry, got:\n{cleared}"
        );

        restore_index_with_env(&ctx, &snapshot, &envs).expect("restore alternate index");

        let restored = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
        assert!(
            restored.lines().any(|l| l == " A alt.txt"),
            "alternate index IT-A marker must survive snapshot/restore, got:\n{restored}"
        );
        let default_after = fs::read(&default_index).expect("read default index after restore");
        assert_eq!(
            default_after, default_before,
            "snapshot/restore with GIT_INDEX_FILE must not overwrite the default index"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_round_trip_preserves_skip_worktree_flag() {
        let (ctx, dir) = fresh_repo("skip-worktree");

        git_ok(&dir, &["update-index", "--skip-worktree", "base.txt"]);
        let before = ls_files_v(&dir);
        assert!(
            before.lines().any(|l| l == "S base.txt"),
            "expected skip-worktree marker pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index(&ctx).expect("snapshot");

        git_ok(&dir, &["update-index", "--no-skip-worktree", "base.txt"]);
        let cleared = ls_files_v(&dir);
        assert!(
            cleared.lines().any(|l| l == "H base.txt"),
            "skip-worktree should clear, got:\n{cleared}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let restored = ls_files_v(&dir);
        assert!(
            restored.lines().any(|l| l == "S base.txt"),
            "skip-worktree flag must survive snapshot/restore round trip, got:\n{restored}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_round_trip_preserves_assume_unchanged_flag() {
        let (ctx, dir) = fresh_repo("assume-unchanged");

        git_ok(&dir, &["update-index", "--assume-unchanged", "base.txt"]);
        let before = ls_files_v(&dir);
        assert!(
            before.lines().any(|l| l == "h base.txt"),
            "expected assume-unchanged marker pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index(&ctx).expect("snapshot");

        git_ok(&dir, &["update-index", "--no-assume-unchanged", "base.txt"]);
        let cleared = ls_files_v(&dir);
        assert!(
            cleared.lines().any(|l| l == "H base.txt"),
            "assume-unchanged should clear, got:\n{cleared}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let restored = ls_files_v(&dir);
        assert!(
            restored.lines().any(|l| l == "h base.txt"),
            "assume-unchanged flag must survive snapshot/restore round trip, got:\n{restored}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_snapshot_drop_removes_backup_file() {
        let (ctx, dir) = fresh_repo("drop-cleanup");

        let backup_path = {
            let snapshot = snapshot_index(&ctx).expect("snapshot");
            let path = snapshot.backup_path().to_path_buf();
            assert!(
                path.exists(),
                "backup must exist before drop: {}",
                path.display()
            );
            path
            // snapshot drops here
        };
        assert!(
            !backup_path.exists(),
            "backup must be removed by Drop: {}",
            backup_path.display()
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
