use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::{
    AppContext, path_exists_or_is_tracked, resolve_git_index_path, run_git,
    run_git_allow_failure_in_with_env, run_git_in_with_env,
};

pub fn prepare_index_for_paths(
    ctx: &AppContext,
    base_index: &Path,
    prepared_index: &Path,
    paths: &[&str],
) -> Result<bool> {
    prepare_index_for_paths_with_options(ctx, base_index, prepared_index, paths, false)
}

pub fn prepare_index_for_paths_force(
    ctx: &AppContext,
    base_index: &Path,
    prepared_index: &Path,
    paths: &[&str],
) -> Result<bool> {
    prepare_index_for_paths_with_options(ctx, base_index, prepared_index, paths, true)
}

fn prepare_index_for_paths_with_options(
    ctx: &AppContext,
    base_index: &Path,
    prepared_index: &Path,
    paths: &[&str],
    force: bool,
) -> Result<bool> {
    if prepared_index.exists() {
        return Err(anyhow!(
            "refusing to overwrite prepared Git index {}",
            prepared_index.display()
        ));
    }
    let paths = eligible_paths(ctx, paths)?;
    if paths.is_empty() {
        return Ok(false);
    }
    if let Some(parent) = prepared_index.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(base_index, prepared_index)?;
    let index = prepared_index
        .to_str()
        .ok_or_else(|| anyhow!("prepared Git index path is not UTF-8"))?;
    let envs = [("GIT_INDEX_FILE", index)];
    for path in &paths {
        let mut args = vec!["add", "-A"];
        if force {
            args.push("-f");
        }
        args.extend(["--", path]);
        run_git_in_with_env(&ctx.root, &envs, &args)?;
    }
    let mut diff_args = vec!["diff", "--cached", "--quiet", "--"];
    diff_args.extend(paths.iter().map(String::as_str));
    let diff = run_git_allow_failure_in_with_env(&ctx.root, &envs, &diff_args)?;
    match diff.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(anyhow!(
            "git {:?} failed: {}",
            diff_args,
            String::from_utf8_lossy(&diff.stderr).trim()
        )),
    }
}

pub fn install_prepared_index_with_guard(
    ctx: &AppContext,
    prepared_index: &Path,
    guard: &dyn Fn(&Path) -> Result<()>,
) -> Result<()> {
    let index = resolve_git_index_path(ctx, &[])?;
    let lock = index_lock_path(&index);
    let mut owns_lock = false;
    let result = (|| {
        let mut source = fs::File::open(prepared_index)?;
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)?;
        owns_lock = true;
        io::copy(&mut source, &mut destination)?;
        destination.sync_all()?;
        drop(destination);
        guard(&lock)?;
        crate::fs_util::rename_atomic(&lock, &index)?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) if !owns_lock => Err(error),
        Err(error) => match fs::remove_file(&lock) {
            Ok(()) => Err(error),
            Err(cleanup) if cleanup.kind() == io::ErrorKind::NotFound => Err(error),
            Err(cleanup) => Err(anyhow!(
                "{error:#}; additionally failed to remove Git index lock '{}': {cleanup}",
                lock.display()
            )),
        },
    }
}

pub fn recover_prepared_index_lock_with_guard(
    ctx: &AppContext,
    prepared_index: &Path,
    guard: &dyn Fn(&Path) -> Result<()>,
) -> Result<bool> {
    let index = resolve_git_index_path(ctx, &[])?;
    let lock = index_lock_path(&index);
    if !lock.exists() {
        return Ok(false);
    }
    if fs::read(&lock)? != fs::read(prepared_index)? {
        return Err(anyhow!(
            "existing Git index lock does not match durable transaction evidence"
        ));
    }
    guard(&lock)?;
    crate::fs_util::rename_atomic(&lock, &index)?;
    Ok(true)
}

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

fn index_lock_path(index: &Path) -> PathBuf {
    let mut name = OsString::from(index.as_os_str());
    name.push(".lock");
    PathBuf::from(name)
}

pub(super) fn eligible_paths(ctx: &AppContext, paths: &[&str]) -> Result<Vec<String>> {
    paths
        .iter()
        .filter_map(|path| match path_exists_or_is_tracked(ctx, path) {
            Ok(true) => Some(Ok((*path).to_string())),
            Ok(false) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
