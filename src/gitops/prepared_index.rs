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
    install_or_recover_prepared_index(ctx, prepared_index, guard, false).map(|_| ())
}

pub fn recover_prepared_index_lock_with_guard(
    ctx: &AppContext,
    prepared_index: &Path,
    guard: &dyn Fn(&Path) -> Result<()>,
) -> Result<bool> {
    install_or_recover_prepared_index(ctx, prepared_index, guard, true)
}

fn install_or_recover_prepared_index(
    ctx: &AppContext,
    prepared_index: &Path,
    guard: &dyn Fn(&Path) -> Result<()>,
    recovery: bool,
) -> Result<bool> {
    let index = resolve_git_index_path(ctx, &[])?;
    let lock = index_lock_path(&index);
    let claim = prepared_index_aux_path(prepared_index, ".lock-claim");
    let capture = prepared_index_aux_path(prepared_index, ".lock-capture");
    let guarded = prepared_index_aux_path(prepared_index, ".lock-guard");
    let publish = prepared_index_aux_path(prepared_index, ".lock-publish");
    let rollback = prepared_index_aux_path(prepared_index, ".lock-rollback");

    if recovery
        && !path_entry_exists(&lock)?
        && !path_entry_exists(&claim)?
        && !path_entry_exists(&capture)?
    {
        return Ok(false);
    }

    let prepared_bytes = read_regular_file(prepared_index, "prepared Git index")?;
    OpenOptions::new()
        .write(true)
        .open(prepared_index)?
        .sync_all()?;
    if path_entry_exists(&claim)? {
        require_regular_exact(&claim, &prepared_bytes, "durable Git index claim")?;
        if recovery && path_entry_exists(&rollback)? {
            if owned_paths_match(&claim, &index, &prepared_bytes)? {
                finish_published_install(&claim, &rollback, &capture, &lock)?;
                remove_private_entry(&guarded)?;
                remove_private_entry(&publish)?;
                return Ok(true);
            }
            if owned_paths_match(&claim, &lock, &prepared_bytes)? {
                remove_private_entry(&rollback)?;
                crate::fs_util::sync_parent_directory(&rollback)?;
            } else {
                rollback_incomplete_publication(&rollback, &capture, &lock, &index)?;
                return Err(anyhow!(
                    "foreign Git index lock was rolled back and preserved"
                ));
            }
        }
        if recovery && owned_paths_match(&claim, &index, &prepared_bytes)? {
            release_owned_lock(&claim, &capture, &lock, &prepared_bytes)?;
            remove_private_entry(&claim)?;
            crate::fs_util::sync_parent_directory(&claim)?;
            return Ok(true);
        }
    }
    reconcile_private_capture(&claim, &capture, &lock, &prepared_bytes)?;
    remove_private_entry(&guarded)?;
    remove_private_entry(&publish)?;
    remove_private_entry(&rollback)?;

    if recovery && !path_entry_exists(&claim)? {
        return Err(anyhow!(
            "existing Git index lock has no durable transaction claim"
        ));
    }

    if !path_entry_exists(&claim)? {
        fs::hard_link(prepared_index, &claim)?;
        crate::fs_util::sync_parent_directory(&claim)?;
    }
    crate::fs_util::write_atomic_bytes(prepared_index, &prepared_bytes)?;
    require_regular_exact(prepared_index, &prepared_bytes, "prepared Git index")?;
    reserve_public_lock(&claim, &capture, &lock, &prepared_bytes)?;

    crate::fs_util::write_atomic_bytes(&guarded, &prepared_bytes)?;
    let result = (|| {
        guard(&guarded)?;
        require_regular_exact(&guarded, &prepared_bytes, "guarded Git index candidate")?;
        require_regular_exact(&claim, &prepared_bytes, "durable Git index claim")?;
        require_regular_exact(prepared_index, &prepared_bytes, "prepared Git index")?;
        fs::hard_link(&index, &rollback)?;
        crate::fs_util::sync_parent_directory(&rollback)?;
        publish_claim_atomic(&claim, &rollback, &capture, &lock, &index, &prepared_bytes)?;
        #[cfg(test)]
        if std::env::var_os("LOOM_TEST_INDEX_INSTALL_CRASH_AFTER_PUBLISH").is_some() {
            std::process::exit(93);
        }
        finish_published_install(&claim, &rollback, &capture, &lock)?;
        Ok(true)
    })();
    remove_private_entry(&guarded)?;
    match result {
        Ok(recovered) => Ok(recovered),
        Err(error) => {
            let cleanup = release_owned_lock(&claim, &capture, &lock, &prepared_bytes);
            match cleanup {
                Ok(()) => {
                    remove_private_entry(&rollback)?;
                    remove_private_entry(&claim)?;
                    crate::fs_util::sync_parent_directory(&claim)?;
                    Err(error)
                }
                Err(cleanup) => Err(anyhow!(
                    "{error:#}; additionally failed to release Git index lock: {cleanup:#}"
                )),
            }
        }
    }
}

#[cfg(not(windows))]
fn publish_claim_atomic(
    claim: &Path,
    _rollback: &Path,
    _capture: &Path,
    lock: &Path,
    index: &Path,
    expected: &[u8],
) -> Result<()> {
    crate::fs_util::exchange_paths_atomic(lock, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    if owned_paths_match(claim, index, expected)? {
        return Ok(());
    }
    crate::fs_util::exchange_paths_atomic(lock, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    Err(anyhow!(
        "foreign Git index lock was atomically rolled back and preserved"
    ))
}

#[cfg(not(windows))]
fn rollback_incomplete_publication(
    rollback: &Path,
    _capture: &Path,
    lock: &Path,
    index: &Path,
) -> Result<()> {
    if same_regular_file(rollback, index)? {
        remove_private_entry(rollback)?;
        crate::fs_util::sync_parent_directory(rollback)?;
        return Ok(());
    }
    if !same_regular_file(rollback, lock)? {
        return Err(anyhow!(
            "interrupted Git index publication has ambiguous rollback evidence"
        ));
    }
    crate::fs_util::exchange_paths_atomic(lock, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    remove_private_entry(rollback)?;
    crate::fs_util::sync_parent_directory(rollback)?;
    Ok(())
}

#[cfg(windows)]
fn rollback_incomplete_publication(
    rollback: &Path,
    capture: &Path,
    lock: &Path,
    index: &Path,
) -> Result<()> {
    if path_entry_exists(capture)? {
        if !same_regular_file(capture, index)? {
            return Err(anyhow!(
                "interrupted Windows Git index publication has ambiguous capture evidence"
            ));
        }
    } else {
        fs::hard_link(index, capture)?;
        crate::fs_util::sync_parent_directory(capture)?;
    }
    crate::fs_util::rename_atomic(rollback, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    crate::fs_util::rename_no_replace_atomic(capture, lock)?;
    crate::fs_util::sync_parent_directory(lock)?;
    Ok(())
}

#[cfg(windows)]
fn publish_claim_atomic(
    claim: &Path,
    rollback: &Path,
    capture: &Path,
    lock: &Path,
    index: &Path,
    expected: &[u8],
) -> Result<()> {
    crate::fs_util::rename_atomic(lock, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    if owned_paths_match(claim, index, expected)? {
        return Ok(());
    }
    fs::hard_link(index, capture)?;
    crate::fs_util::sync_parent_directory(capture)?;
    crate::fs_util::rename_atomic(rollback, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    crate::fs_util::rename_no_replace_atomic(capture, lock)?;
    crate::fs_util::sync_parent_directory(lock)?;
    Err(anyhow!(
        "foreign Git index lock was atomically rolled back and preserved"
    ))
}

fn finish_published_install(
    claim: &Path,
    rollback: &Path,
    capture: &Path,
    lock: &Path,
) -> Result<()> {
    if path_entry_exists(capture)? {
        if !same_regular_file(rollback, capture)? {
            return Err(anyhow!(
                "captured post-publication Git index is not the durable rollback entry"
            ));
        }
    } else if path_entry_exists(lock)? {
        crate::fs_util::rename_no_replace_atomic(lock, capture)?;
        crate::fs_util::sync_parent_directory(lock)?;
        if !same_regular_file(rollback, capture)? {
            restore_foreign_capture(capture, lock)?;
            return Err(anyhow!(
                "post-publication Git index lock was concurrently replaced"
            ));
        }
    }
    remove_private_entry(capture)?;
    remove_private_entry(rollback)?;
    remove_private_entry(claim)?;
    crate::fs_util::sync_parent_directory(claim)?;
    Ok(())
}

fn reserve_public_lock(claim: &Path, capture: &Path, lock: &Path, expected: &[u8]) -> Result<()> {
    match fs::hard_link(claim, lock) {
        Ok(()) => {
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            capture_and_remove_owned_lock(claim, capture, lock, expected)?;
            fs::hard_link(claim, lock)?;
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn capture_and_remove_owned_lock(
    claim: &Path,
    capture: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    capture_owned_lock(claim, capture, lock, expected)?;
    remove_private_entry(capture)?;
    crate::fs_util::sync_parent_directory(capture)?;
    Ok(())
}

fn capture_owned_lock(claim: &Path, capture: &Path, lock: &Path, expected: &[u8]) -> Result<()> {
    crate::fs_util::rename_no_replace_atomic(lock, capture)?;
    crate::fs_util::sync_parent_directory(lock)?;
    if captured_lock_is_owned(claim, capture, expected)? {
        return Ok(());
    }
    restore_foreign_capture(capture, lock)?;
    Err(anyhow!(
        "existing Git index lock is not owned by the durable transaction claim"
    ))
}

fn release_owned_lock(claim: &Path, capture: &Path, lock: &Path, expected: &[u8]) -> Result<()> {
    if path_entry_exists(capture)? {
        if captured_lock_is_owned(claim, capture, expected)? {
            remove_private_entry(capture)?;
            crate::fs_util::sync_parent_directory(capture)?;
            return Ok(());
        }
        restore_foreign_capture(capture, lock)?;
        return Err(anyhow!(
            "captured Git index lock is not owned by the durable transaction claim"
        ));
    }
    if path_entry_exists(lock)? {
        return capture_and_remove_owned_lock(claim, capture, lock, expected);
    }
    Ok(())
}

fn reconcile_private_capture(
    claim: &Path,
    capture: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    if !path_entry_exists(capture)? {
        return Ok(());
    }
    if path_entry_exists(claim)? && captured_lock_is_owned(claim, capture, expected)? {
        remove_private_entry(capture)?;
        crate::fs_util::sync_parent_directory(capture)?;
        return Ok(());
    }
    restore_foreign_capture(capture, lock)?;
    Err(anyhow!(
        "captured Git index lock is not owned by the durable transaction claim"
    ))
}

fn restore_foreign_capture(capture: &Path, lock: &Path) -> Result<()> {
    match crate::fs_util::rename_no_replace_atomic(capture, lock) {
        Ok(()) => {
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(anyhow!(
            "foreign Git index lock restoration collided; both lock entries were preserved"
        )),
        Err(error) => Err(error.into()),
    }
}

fn captured_lock_is_owned(claim: &Path, capture: &Path, expected: &[u8]) -> Result<bool> {
    let claim_metadata = fs::symlink_metadata(claim)?;
    let capture_metadata = fs::symlink_metadata(capture)?;
    Ok(claim_metadata.file_type().is_file()
        && capture_metadata.file_type().is_file()
        && same_file_identity(&claim_metadata, &capture_metadata)
        && fs::read(claim)? == expected
        && fs::read(capture)? == expected)
}

fn owned_paths_match(claim: &Path, candidate: &Path, expected: &[u8]) -> Result<bool> {
    if !path_entry_exists(candidate)? {
        return Ok(false);
    }
    captured_lock_is_owned(claim, candidate, expected)
}

fn same_regular_file(left: &Path, right: &Path) -> Result<bool> {
    if !path_entry_exists(left)? || !path_entry_exists(right)? {
        return Ok(false);
    }
    let left = fs::symlink_metadata(left)?;
    let right = fs::symlink_metadata(right)?;
    Ok(left.file_type().is_file()
        && right.file_type().is_file()
        && same_file_identity(&left, &right))
}

fn read_regular_file(path: &Path, description: &str) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(anyhow!("{description} is not a regular file"));
    }
    Ok(fs::read(path)?)
}

fn require_regular_exact(path: &Path, expected: &[u8], description: &str) -> Result<()> {
    if read_regular_file(path, description)? != expected {
        return Err(anyhow!("{description} changed during guarded installation"));
    }
    Ok(())
}

fn remove_private_entry(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn path_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn prepared_index_aux_path(prepared_index: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(prepared_index.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}

pub fn prepared_index_claim_exists(prepared_index: &Path) -> Result<bool> {
    path_entry_exists(&prepared_index_aux_path(prepared_index, ".lock-claim"))
}

#[cfg(test)]
pub(super) fn seed_owned_index_lock(prepared: &Path, lock: &Path) {
    let bytes = fs::read(prepared).expect("prepared index bytes");
    let claim = prepared_index_aux_path(prepared, ".lock-claim");
    fs::hard_link(prepared, &claim).expect("durable index claim");
    crate::fs_util::write_atomic_bytes(prepared, &bytes).expect("detach prepared evidence");
    fs::hard_link(&claim, lock).expect("publish owned index lock");
}

#[cfg(test)]
pub(super) fn assert_no_index_aux_paths(prepared: &Path) {
    for suffix in [
        ".lock-claim",
        ".lock-capture",
        ".lock-guard",
        ".lock-publish",
        ".lock-rollback",
    ] {
        assert!(
            !prepared_index_aux_path(prepared, suffix).exists(),
            "private index path remained after completion: {suffix}"
        );
    }
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    left.volume_serial_number().is_some()
        && left.volume_serial_number() == right.volume_serial_number()
        && left.file_index().is_some()
        && left.file_index() == right.file_index()
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    false
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
