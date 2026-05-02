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

/// Captured index state used to restore staging after a failed mutation.
///
/// `tree` is the git tree object produced by `git write-tree`; replaying it
/// with `git read-tree` rebuilds every fully-staged blob. `intent_to_add`
/// holds paths whose index entry has the all-zero blob (`git add -N`), since
/// those entries cannot be encoded as a tree and would otherwise become
/// untracked after `read-tree`.
#[derive(Debug, Clone)]
pub struct IndexSnapshot {
    pub tree: String,
    pub intent_to_add: Vec<String>,
}

pub fn snapshot_index(ctx: &AppContext) -> Result<IndexSnapshot> {
    // Capture intent-to-add paths *before* `write-tree` so a future change to
    // either step cannot silently drop entries between snapshot and replay.
    let intent_to_add = intent_to_add_paths(ctx)?;
    let tree = run_git(ctx, &["write-tree"])?;
    Ok(IndexSnapshot {
        tree,
        intent_to_add,
    })
}

pub fn restore_index(ctx: &AppContext, snapshot: &IndexSnapshot) -> Result<()> {
    run_git(ctx, &["read-tree", &snapshot.tree])?;
    for path in &snapshot.intent_to_add {
        // `git add --intent-to-add` is the only stable CLI form; plumbing
        // `update-index` has no equivalent flag. The path must exist in the
        // working tree, which is true on the rollback path because read-tree
        // does not touch the worktree.
        run_git(ctx, &["add", "--intent-to-add", "--", path])?;
    }
    Ok(())
}

fn intent_to_add_paths(ctx: &AppContext) -> Result<Vec<String>> {
    // `git status --porcelain=v1 -z -uno` emits one NUL-terminated record per
    // tracked-or-staged path. IT-A entries have X=' ' Y='A' (the index has
    // the entry as an empty blob with the CE_INTENT_TO_ADD flag set), while
    // real `git add` entries report X='A'. Encoding stays stable across git
    // versions, unlike `ls-files -s` whose blob sha collides with empty
    // files. `-uno` skips untracked paths since they cannot carry IT-A.
    //
    // We bypass `run_git`'s trim() because the IT-A record's leading SPACE is
    // the load-bearing signal — trimming it shifts X to 'A' and misclassifies
    // the entry as a real staged add.
    let output = run_git_allow_failure(ctx, &["status", "--porcelain=v1", "-z", "-uno"])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git status (intent-to-add probe) failed: {}", stderr));
    }
    let mut paths = Vec::new();
    let mut iter = output.stdout.split(|&b| b == 0);
    while let Some(record) = iter.next() {
        if record.is_empty() {
            continue;
        }
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        let x = record[0];
        let y = record[1];
        // Rename/copy records (X in {'R','C'}) carry the `from` path in the
        // next NUL-terminated field; consume it so iteration stays aligned.
        if matches!(x, b'R' | b'C') {
            if iter.next().is_none() {
                break;
            }
            continue;
        }
        if x == b' ' && y == b'A' {
            paths.push(String::from_utf8_lossy(&record[3..]).into_owned());
        }
    }
    Ok(paths)
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
            ["commit", "--allow-empty", "-q", "-m", "init"].as_slice(),
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
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
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

    #[test]
    fn intent_to_add_paths_returns_only_intent_to_add_entries() {
        let (ctx, dir) = fresh_repo("ita-detect");

        fs::write(dir.join("ita.txt"), "stand-in").expect("write ita");
        git_ok(&dir, &["add", "-N", "--", "ita.txt"]);

        fs::write(dir.join("real.txt"), "real").expect("write real");
        git_ok(&dir, &["add", "--", "real.txt"]);

        let paths = intent_to_add_paths(&ctx).expect("query ita");
        assert_eq!(paths, vec!["ita.txt".to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_index_recovers_intent_to_add_after_clobber() {
        let (ctx, dir) = fresh_repo("ita-restore");

        fs::write(dir.join("ita.txt"), "stand-in").expect("write ita");
        git_ok(&dir, &["add", "-N", "--", "ita.txt"]);

        let snapshot = snapshot_index(&ctx).expect("snapshot");
        assert_eq!(
            snapshot.intent_to_add,
            vec!["ita.txt".to_string()],
            "snapshot must capture IT-A before clobber"
        );

        // Wipe the index entry: this is what `git read-tree` of a tree without
        // the IT-A path would do during a real rollback.
        git_ok(&dir, &["update-index", "--force-remove", "ita.txt"]);
        let after_clobber = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            after_clobber
                .lines()
                .any(|line| line == "?? ita.txt"),
            "post-clobber file must be untracked, got:\n{after_clobber}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let after_restore = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            after_restore.lines().any(|line| line == " A ita.txt"),
            "restore must reinstate IT-A marker (' A path'), got:\n{after_restore}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_index_returns_empty_intent_to_add_for_clean_repo() {
        let (ctx, dir) = fresh_repo("ita-empty");
        let snapshot = snapshot_index(&ctx).expect("snapshot");
        assert!(snapshot.intent_to_add.is_empty());
        assert!(!snapshot.tree.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
