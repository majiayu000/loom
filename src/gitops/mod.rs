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

#[derive(Debug, Clone, Copy)]
enum GitEnvMode {
    Normal,
    Restricted,
}

fn run_git_raw_in_with_env_and_input(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    input: Option<&[u8]>,
    args: &[&str],
) -> Result<Output> {
    run_git_raw_in_with_env_mode_and_input(repo_dir, envs, input, args, GitEnvMode::Normal)
}

fn run_git_raw_in_with_env_mode_and_input(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    input: Option<&[u8]>,
    args: &[&str],
    env_mode: GitEnvMode,
) -> Result<Output> {
    let mut command = Command::new("git");
    if matches!(env_mode, GitEnvMode::Restricted) {
        command.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0");
    }
    command
        .current_dir(repo_dir)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg("protocol.ssh.allow=always")
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

pub fn run_git_allow_failure_restricted(ctx: &AppContext, args: &[&str]) -> Result<Output> {
    run_git_raw_in_with_env_mode_and_input(&ctx.root, &[], None, args, GitEnvMode::Restricted)
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

pub fn stage_path(ctx: &AppContext, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["add", "--", &path_str])?;
    Ok(())
}

pub fn commit(ctx: &AppContext, message: &str) -> Result<String> {
    run_git(ctx, &["commit", "-m", message])?;
    head(ctx)
}

pub fn commit_paths_if_changed(
    ctx: &AppContext,
    paths: &[&str],
    message: &str,
) -> Result<Option<String>> {
    let paths = paths
        .iter()
        .filter_map(|path| match path_exists_or_is_tracked(ctx, path) {
            Ok(true) => Some(Ok((*path).to_string())),
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<Result<Vec<_>>>()?;

    if paths.is_empty() {
        return Ok(None);
    }

    for path in &paths {
        run_git(ctx, &["add", "-A", "--", path])?;
    }

    let mut diff_args = vec!["diff", "--cached", "--quiet", "--"];
    diff_args.extend(paths.iter().map(String::as_str));
    let diff = run_git_allow_failure(ctx, &diff_args)?;
    if diff.status.success() {
        return Ok(None);
    }

    let mut commit_args = vec!["commit", "-m", message, "--"];
    commit_args.extend(paths.iter().map(String::as_str));
    run_git(ctx, &commit_args)?;
    head(ctx).map(Some)
}

fn path_exists_or_is_tracked(ctx: &AppContext, path: &str) -> Result<bool> {
    if ctx.root.join(path).exists() {
        return Ok(true);
    }

    let output = run_git_allow_failure(ctx, &["ls-files", "--error-unmatch", "--", path])?;
    Ok(output.status.success())
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
    validate_git_url(url)?;
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
    ensure_origin_remote_url_allowed(ctx)?;
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
    ensure_origin_remote_url_allowed(ctx)?;
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
    ensure_origin_remote_url_allowed(ctx)?;
    let mut args = vec!["push", "--atomic", "origin", "HEAD:main"];
    if history_branch_exists(ctx)? {
        args.push("loom-history:loom-history");
    }
    args.push("--tags");
    run_git(ctx, &args)?;
    Ok(())
}

pub fn pull_rebase_main(ctx: &AppContext) -> Result<()> {
    ensure_origin_remote_url_allowed(ctx)?;
    let output = run_git_allow_failure(ctx, &["pull", "--rebase", "origin", "main"])?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let _ = run_git_allow_failure(ctx, &["rebase", "--abort"]);

    Err(anyhow!("git pull --rebase origin main failed: {}", stderr))
}

fn ensure_origin_remote_url_allowed(ctx: &AppContext) -> Result<()> {
    if let Some(url) = remote_url(ctx)? {
        validate_git_url(&url)?;
    }
    Ok(())
}

pub fn validate_git_url(raw: &str) -> Result<()> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(anyhow!("git url must not be empty"));
    }
    if url != raw {
        return Err(anyhow!(
            "git url must not include leading or trailing whitespace"
        ));
    }
    if url.starts_with('-') {
        return Err(anyhow!("git url must not start with '-'"));
    }
    if url
        .chars()
        .any(|ch| ch.is_ascii_control() || ch.is_whitespace())
    {
        return Err(anyhow!(
            "git url must not contain whitespace or control characters"
        ));
    }
    if let Some((scheme, _rest)) = url.split_once("://") {
        return match scheme {
            "https" | "ssh" => Ok(()),
            _ => Err(anyhow!(
                "unsupported git url scheme '{}'; use https:// or ssh://",
                scheme
            )),
        };
    }
    validate_scp_like_git_url(url)
}

fn validate_scp_like_git_url(url: &str) -> Result<()> {
    let Some((user_host, path)) = url.split_once(':') else {
        return Err(anyhow!(
            "git url must use https://, ssh://, or git@host:owner/repo.git"
        ));
    };
    if user_host.is_empty() || path.is_empty() || path.starts_with(':') {
        return Err(anyhow!(
            "git url must use https://, ssh://, or git@host:owner/repo.git"
        ));
    }
    if user_host.contains('/') || user_host == "ext" {
        return Err(anyhow!("unsupported git url"));
    }
    let host = user_host
        .rsplit_once('@')
        .map_or(user_host, |(_, host)| host);
    if host.is_empty() || !host.contains('.') {
        return Err(anyhow!("scp-like git url must include a hostname"));
    }
    if !host
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
    {
        return Err(anyhow!("scp-like git url contains an invalid hostname"));
    }
    Ok(())
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
    use super::validate_git_url;

    #[test]
    fn git_url_validation_accepts_https_and_ssh_forms() {
        validate_git_url("https://github.com/org/repo.git").expect("https accepted");
        validate_git_url("ssh://git@github.com/org/repo.git").expect("ssh accepted");
        validate_git_url("git@github.com:org/repo.git").expect("scp-like ssh accepted");
    }

    #[test]
    fn git_url_validation_rejects_dangerous_protocols_and_options() {
        for url in [
            "ext::sh -c 'touch /tmp/pwned'",
            "file:///etc/passwd",
            "--upload-pack=sh",
            "git://github.com/org/repo.git",
            " https://github.com/org/repo.git",
        ] {
            assert!(validate_git_url(url).is_err(), "{url} should be rejected");
        }
    }
}
