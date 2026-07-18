use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use super::{
    AppContext, head, path_exists_or_is_tracked, resolve_git_index_path, run_git,
    run_git_allow_failure_in_with_env, run_git_in_with_env,
};

pub fn prepare_index_for_paths(
    ctx: &AppContext,
    base_index: &Path,
    prepared_index: &Path,
    paths: &[&str],
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
        run_git_in_with_env(&ctx.root, &envs, &["add", "-A", "--", path])?;
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

pub fn install_prepared_index_with_guard<F>(
    ctx: &AppContext,
    prepared_index: &Path,
    guard: F,
) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    let index = resolve_git_index_path(ctx, &[])?;
    let lock = index_lock_path(&index);
    let result = (|| {
        let mut source = fs::File::open(prepared_index)?;
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)?;
        io::copy(&mut source, &mut destination)?;
        destination.sync_all()?;
        drop(destination);
        guard(&lock)?;
        crate::fs_util::rename_atomic(&lock, &index)?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(()),
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

pub fn commit_prepared_paths(
    ctx: &AppContext,
    paths: &[&str],
    message: &str,
) -> Result<Option<String>> {
    let paths = eligible_paths(ctx, paths)?;
    if paths.is_empty() {
        return Ok(None);
    }
    let mut diff_args = vec!["diff", "--cached", "--quiet", "--"];
    diff_args.extend(paths.iter().map(String::as_str));
    let diff = super::run_git_allow_failure(ctx, &diff_args)?;
    match diff.status.code() {
        Some(0) => return Ok(None),
        Some(1) => {}
        _ => {
            return Err(anyhow!(
                "git {:?} failed: {}",
                diff_args,
                String::from_utf8_lossy(&diff.stderr).trim()
            ));
        }
    }
    let mut commit_args = vec!["commit", "-m", message, "--"];
    commit_args.extend(paths.iter().map(String::as_str));
    run_git(ctx, &commit_args)?;
    head(ctx).map(Some)
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
