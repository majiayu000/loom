use anyhow::{Result, anyhow};

use crate::state::AppContext;

use super::{HISTORY_BRANCH, HISTORY_BRANCH_REF, ORIGIN_HISTORY_BRANCH_REF};
use super::exec::{run_git, run_git_allow_failure};
use super::url::validate_git_url;

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
        .map_err(|_| anyhow!("failed to parse left-only count"))?;
    let right_only = parts
        .next()
        .ok_or_else(|| anyhow!("unexpected rev-list output"))?
        .parse::<u32>()
        .map_err(|_| anyhow!("failed to parse right-only count"))?;
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
