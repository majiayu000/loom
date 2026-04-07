use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow};

use crate::state::AppContext;

fn run_git_raw(ctx: &AppContext, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .current_dir(&ctx.root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))?;
    Ok(output)
}

pub fn run_git(ctx: &AppContext, args: &[&str]) -> Result<String> {
    let output = run_git_raw(ctx, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {:?} failed: {}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_git_allow_failure(ctx: &AppContext, args: &[&str]) -> Result<Output> {
    run_git_raw(ctx, args)
}

pub fn ensure_repo_initialized(ctx: &AppContext) -> Result<()> {
    if ctx.root.join(".git").exists() {
        return Ok(());
    }

    let init_main = run_git_allow_failure(ctx, &["init", "-b", "main"])?;
    if !init_main.status.success() {
        run_git(ctx, &["init"])?;
        let _ = run_git_allow_failure(ctx, &["branch", "-M", "main"])?;
    }

    let _ = run_git_allow_failure(ctx, &["config", "user.name", "loom"])?;
    let _ = run_git_allow_failure(ctx, &["config", "user.email", "loom@local"])?;

    Ok(())
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

pub fn head(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["rev-parse", "HEAD"])
}

pub fn short_head(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["rev-parse", "--short", "HEAD"])
}

pub fn create_tag(ctx: &AppContext, tag: &str) -> Result<()> {
    run_git(ctx, &["tag", tag])?;
    Ok(())
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

pub fn fetch_origin_main(ctx: &AppContext) -> Result<()> {
    run_git(ctx, &["fetch", "origin", "main"])?;
    Ok(())
}

pub fn ahead_behind_main(ctx: &AppContext) -> Result<(u32, u32)> {
    let output = run_git(
        ctx,
        &["rev-list", "--left-right", "--count", "origin/main...HEAD"],
    )?;
    let mut parts = output.split_whitespace();
    let behind = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output"))?
        .parse::<u32>()
        .context("failed to parse behind count")?;
    let ahead = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output"))?
        .parse::<u32>()
        .context("failed to parse ahead count")?;
    Ok((ahead, behind))
}

pub fn push_main_with_tags(ctx: &AppContext) -> Result<()> {
    run_git(ctx, &["push", "--follow-tags", "origin", "HEAD:main"])?;
    Ok(())
}

pub fn pull_rebase_main(ctx: &AppContext) -> Result<()> {
    run_git(ctx, &["pull", "--rebase", "origin", "main"])?;
    Ok(())
}

pub fn diff_path(ctx: &AppContext, from: &str, to: &str, path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["diff", from, to, "--", &path_str])
}

pub fn fsck(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["fsck", "--no-progress"])
}
