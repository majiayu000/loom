use std::path::Path;

use anyhow::Result;

use crate::state::AppContext;

use super::exec::{run_git, run_git_allow_failure};
use super::repo::head;

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
