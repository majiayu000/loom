use std::path::Path;

use anyhow::{Result, anyhow};

use crate::state::AppContext;

use super::exec::{
    run_git, run_git_allow_failure, run_git_allow_failure_in, run_git_in,
};

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

pub(crate) fn ensure_local_identity(ctx: &AppContext) -> Result<()> {
    ensure_local_identity_in(&ctx.root)
}

pub(crate) fn ensure_local_identity_in(repo_dir: &Path) -> Result<()> {
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
