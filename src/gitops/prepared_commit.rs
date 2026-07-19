use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow};

use super::prepared_index_paths::eligible_paths;
use super::{AppContext, run_git, run_git_in_with_env};

pub fn create_prepared_commit(
    ctx: &AppContext,
    prepared_index: &Path,
    commit_index: &Path,
    paths: &[&str],
    parent: &str,
    message: &str,
) -> Result<String> {
    create_prepared_commit_inner(
        ctx,
        prepared_index,
        commit_index,
        paths,
        parent,
        message,
        false,
    )
}

pub fn create_prepared_commit_retaining_index(
    ctx: &AppContext,
    prepared_index: &Path,
    commit_index: &Path,
    paths: &[&str],
    parent: &str,
    message: &str,
) -> Result<String> {
    create_prepared_commit_inner(
        ctx,
        prepared_index,
        commit_index,
        paths,
        parent,
        message,
        true,
    )
}

fn create_prepared_commit_inner(
    ctx: &AppContext,
    prepared_index: &Path,
    commit_index: &Path,
    paths: &[&str],
    parent: &str,
    message: &str,
    retain_index: bool,
) -> Result<String> {
    let paths = eligible_paths(ctx, paths)?;
    if paths.is_empty() {
        return Err(anyhow!("prepared commit has no eligible paths"));
    }
    if commit_index.exists() {
        return Err(anyhow!(
            "refusing to overwrite prepared commit index {}",
            commit_index.display()
        ));
    }
    fs::copy(prepared_index, commit_index)?;
    let index = commit_index
        .to_str()
        .ok_or_else(|| anyhow!("prepared commit index path is not UTF-8"))?;
    let envs = [("GIT_INDEX_FILE", index)];
    let mut reset_args = vec![
        "reset".to_string(),
        "-q".to_string(),
        parent.to_string(),
        "--".to_string(),
        ".".to_string(),
    ];
    reset_args.extend(
        paths
            .iter()
            .map(|path| format!(":(top,exclude,literal){path}")),
    );
    let reset_refs = reset_args.iter().map(String::as_str).collect::<Vec<_>>();
    let result = (|| {
        run_git_in_with_env(&ctx.root, &envs, &reset_refs)?;
        let tree = run_git_in_with_env(&ctx.root, &envs, &["write-tree"])?;
        super::ensure_local_identity(ctx)?;
        run_git(ctx, &["commit-tree", &tree, "-p", parent, "-m", message])
    })();
    if retain_index {
        if result.is_ok() {
            crate::fs_util::sync_file_and_parent(commit_index)?;
        }
        return result;
    }
    match (result, fs::remove_file(commit_index)) {
        (Ok(commit), Ok(())) => Ok(commit),
        (Ok(_), Err(cleanup)) => Err(anyhow!(
            "failed to remove prepared commit index '{}': {cleanup}",
            commit_index.display()
        )),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(anyhow!(
            "{error:#}; additionally failed to remove prepared commit index '{}': {cleanup}",
            commit_index.display()
        )),
    }
}

pub fn move_head_if_unchanged(ctx: &AppContext, commit: &str, expected: &str) -> Result<()> {
    run_git(ctx, &["update-ref", "HEAD", commit, expected])?;
    Ok(())
}
