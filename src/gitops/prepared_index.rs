use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Result, anyhow};

use super::prepared_index_paths::{eligible_paths, index_lock_path, prepared_index_aux_path};
use super::{
    AppContext, resolve_git_index_path, run_git_allow_failure_in_with_env, run_git_in_with_env,
};

const INDEX_LOCK_PLACEHOLDER: &[u8] = b"loom index lock placeholder\n";

#[derive(Debug)]
struct PreparedIndexLockRetained(String);

impl std::fmt::Display for PreparedIndexLockRetained {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PreparedIndexLockRetained {}

pub(crate) fn prepared_index_lock_was_retained(error: &anyhow::Error) -> bool {
    error.downcast_ref::<PreparedIndexLockRetained>().is_some()
}

fn retained_lock_error(lock: &Path, error: anyhow::Error) -> anyhow::Error {
    PreparedIndexLockRetained(format!(
        "{error:#}; published Git index lock '{}' was retained for recovery",
        lock.display()
    ))
    .into()
}

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

/// Safely release a retained lock after the surrounding transaction has
/// proved that its prepared index must be abandoned.
pub fn discard_prepared_index_lock(ctx: &AppContext, prepared_index: &Path) -> Result<bool> {
    let index = resolve_git_index_path(ctx, &[])?;
    let lock = index_lock_path(&index);
    let claim = prepared_index_aux_path(ctx, prepared_index, ".lock-claim")?;
    if !path_entry_exists(&claim)? {
        return Ok(false);
    }
    let capture = prepared_index_aux_path(ctx, prepared_index, ".lock-capture")?;
    let sentinel = prepared_index_aux_path(ctx, prepared_index, ".lock-sentinel")?;
    let guarded = prepared_index_aux_path(ctx, prepared_index, ".lock-guard")?;
    let publish = prepared_index_aux_path(ctx, prepared_index, ".lock-publish")?;
    let expected = read_regular_file(&claim, "durable Git index claim")
        .map_err(|error| retained_lock_error(&lock, error))?;
    let result = (|| {
        reconcile_legacy_sentinel(&claim, &capture, &sentinel, &lock, &expected)?;
        if path_entry_exists(&capture)? {
            if !captured_lock_is_owned(&claim, &capture, &expected)? {
                return Err(anyhow!("unknown captured Git index lock was preserved"));
            }
            require_owned_placeholder(&lock, &sentinel)?;
            remove_private_entry(&capture)?;
            crate::fs_util::sync_parent_directory(&capture)?;
        }
        if path_entry_exists(&lock)? {
            clear_completed_public_lock(&claim, &capture, &sentinel, &lock, &expected)?;
        }
        remove_private_entry(&guarded)?;
        remove_private_entry(&publish)?;
        remove_private_entry(&sentinel)?;
        remove_private_entry(&claim)?;
        crate::fs_util::sync_parent_directory(&claim)?;
        Ok(true)
    })();
    result.map_err(|error| retained_lock_error(&lock, error))
}

fn install_or_recover_prepared_index(
    ctx: &AppContext,
    prepared_index: &Path,
    guard: &dyn Fn(&Path) -> Result<()>,
    recovery: bool,
) -> Result<bool> {
    let index = resolve_git_index_path(ctx, &[])?;
    let lock = index_lock_path(&index);
    let claim = prepared_index_aux_path(ctx, prepared_index, ".lock-claim")?;
    let capture = prepared_index_aux_path(ctx, prepared_index, ".lock-capture")?;
    let sentinel = prepared_index_aux_path(ctx, prepared_index, ".lock-sentinel")?;
    let guarded = prepared_index_aux_path(ctx, prepared_index, ".lock-guard")?;
    let publish = prepared_index_aux_path(ctx, prepared_index, ".lock-publish")?;

    if recovery
        && !path_entry_exists(&lock)?
        && !path_entry_exists(&claim)?
        && !path_entry_exists(&capture)?
        && !path_entry_exists(&sentinel)?
    {
        return Ok(false);
    }
    let prepared_bytes = read_regular_file(prepared_index, "prepared Git index")?;
    crate::fs_util::sync_file_and_parent(prepared_index)?;
    remove_private_entry(&guarded)?;
    remove_private_entry(&publish)?;
    if let Err(error) =
        reconcile_legacy_sentinel(&claim, &capture, &sentinel, &lock, &prepared_bytes)
    {
        return Err(retained_lock_error(&lock, error));
    }

    if recovery {
        match finish_completed_index_install(
            &index,
            &claim,
            &capture,
            &sentinel,
            &lock,
            &prepared_bytes,
        ) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(retained_lock_error(&lock, error)),
        }
        match finish_captured_index_install(
            &index,
            &claim,
            &capture,
            &sentinel,
            &lock,
            &prepared_bytes,
        ) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(retained_lock_error(&lock, error)),
        }
    }
    if let Err(error) =
        reconcile_private_capture(&claim, &capture, &sentinel, &lock, &prepared_bytes)
    {
        return Err(retained_lock_error(&lock, error));
    }

    if recovery && !path_entry_exists(&claim)? {
        return Err(anyhow!(
            "existing Git index lock has no durable transaction claim"
        ));
    }

    create_or_validate_claim(&claim, &prepared_bytes)?;
    crate::fs_util::write_atomic_bytes(prepared_index, &prepared_bytes)?;
    require_regular_exact(prepared_index, &prepared_bytes, "prepared Git index")?;
    if let Err(error) = reserve_public_lock(&claim, &lock, &prepared_bytes) {
        return match public_lock_is_owned(&claim, &lock, &prepared_bytes) {
            Ok(true) => Err(retained_lock_error(&lock, error)),
            Ok(false) => Err(error),
            Err(inspect) => Err(retained_lock_error(
                &lock,
                anyhow!("{error:#}; additionally failed to inspect published lock: {inspect:#}"),
            )),
        };
    }

    let result = (|| {
        injected_index_failure("before_guard_create")?;
        crate::fs_util::write_atomic_bytes(&guarded, &prepared_bytes)?;
        #[cfg(debug_assertions)]
        if std::env::var_os("LOOM_TEST_PREPARED_INDEX_FAIL_AFTER_PUBLICATION").is_some() {
            return Err(anyhow!("prepared index post-publication test failure"));
        }
        #[cfg(debug_assertions)]
        if std::env::var_os("LOOM_TEST_ROLLBACK_INDEX_FAIL_AFTER_PUBLICATION").is_some()
            && prepared_index
                .file_name()
                .is_some_and(|name| name == "index")
        {
            return Err(anyhow!("rollback index post-publication test failure"));
        }
        guard(&guarded)?;
        require_regular_exact(&guarded, &prepared_bytes, "guarded Git index candidate")?;
        require_regular_exact(&claim, &prepared_bytes, "durable Git index claim")?;
        require_regular_exact(prepared_index, &prepared_bytes, "prepared Git index")?;
        #[cfg(debug_assertions)]
        if std::env::var_os("LOOM_TEST_REGISTRY_INDEX_FAIL_AFTER_GUARD").is_some()
            && prepared_index
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("registry-"))
        {
            return Err(anyhow!("registry index post-guard test failure"));
        }
        publish_claimed_index(
            &claim,
            &capture,
            &sentinel,
            &lock,
            &publish,
            &index,
            &prepared_bytes,
        )?;
        remove_private_entry(&guarded)?;
        remove_private_entry(&claim)?;
        crate::fs_util::sync_parent_directory(&claim)?;
        injected_index_failure("after_claim_remove")?;
        injected_index_crash("after_claim_remove");
        Ok(true)
    })();
    let cleanup =
        injected_index_failure("guard_cleanup").and_then(|()| remove_private_entry(&guarded));
    match (result, cleanup) {
        (Ok(recovered), Ok(())) => Ok(recovered),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(error), Ok(())) => Err(retained_lock_error(&lock, error)),
        (Err(error), Err(cleanup)) => Err(retained_lock_error(
            &lock,
            anyhow!("{error:#}; additionally failed to clean guard candidate: {cleanup:#}"),
        )),
    }
}

fn finish_completed_index_install(
    index: &Path,
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<bool> {
    if !path_entry_exists(index)? || !path_entry_exists(claim)? {
        return Ok(false);
    }
    if !crate::fs_util::same_file_identity_paths(index, claim)?
        || fs::read(index)? != expected
        || fs::read(claim)? != expected
    {
        return Ok(false);
    }
    if path_entry_exists(lock)? {
        clear_completed_public_lock(claim, capture, sentinel, lock, expected)?;
    }
    remove_private_entry(capture)?;
    remove_private_entry(sentinel)?;
    remove_private_entry(claim)?;
    crate::fs_util::sync_parent_directory(claim)?;
    Ok(true)
}

fn finish_captured_index_install(
    index: &Path,
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<bool> {
    if !path_entry_exists(claim)?
        || !path_entry_exists(capture)?
        || !captured_lock_is_owned(claim, capture, expected)?
    {
        return Ok(false);
    }
    require_owned_placeholder(lock, sentinel)?;
    crate::fs_util::rename_atomic(capture, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    remove_placeholder_lock(lock, sentinel)?;
    remove_private_entry(claim)?;
    crate::fs_util::sync_parent_directory(claim)?;
    Ok(true)
}

fn create_or_validate_claim(claim: &Path, prepared_bytes: &[u8]) -> Result<()> {
    if path_entry_exists(claim)? {
        return require_regular_exact(claim, prepared_bytes, "durable Git index claim");
    }
    let parent = claim
        .parent()
        .ok_or_else(|| anyhow!("Git index claim has no parent: {}", claim.display()))?;
    let staging = parent.join(format!(
        ".loom-index-claim-staging-{}",
        uuid::Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    file.write_all(prepared_bytes)?;
    file.sync_all()?;
    drop(file);
    crate::fs_util::sync_parent_directory(&staging)?;
    match crate::fs_util::rename_no_replace_atomic(&staging, claim) {
        Ok(()) => Ok(crate::fs_util::sync_parent_directory(claim)?),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            remove_private_entry(&staging)?;
            require_regular_exact(claim, prepared_bytes, "durable Git index claim")
        }
        Err(error) => {
            let cleanup = remove_private_entry(&staging);
            match cleanup {
                Ok(()) => Err(error.into()),
                Err(cleanup) => Err(anyhow!(
                    "{error}; additionally failed to remove Git index claim staging '{}': {cleanup:#}",
                    staging.display()
                )),
            }
        }
    }
}

fn reserve_public_lock(claim: &Path, lock: &Path, expected: &[u8]) -> Result<()> {
    match fs::hard_link(claim, lock) {
        Ok(()) => {
            injected_index_failure("after_lock_link")?;
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if public_lock_is_owned(claim, lock, expected)? {
                Ok(())
            } else {
                Err(anyhow!(
                    "existing Git index lock is not owned by the durable transaction claim"
                ))
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn public_lock_is_owned(claim: &Path, lock: &Path, expected: &[u8]) -> Result<bool> {
    if !path_entry_exists(claim)? || !path_entry_exists(lock)? {
        return Ok(false);
    }
    Ok(crate::fs_util::same_file_identity_paths(claim, lock)?
        && fs::read(claim)? == expected
        && fs::read(lock)? == expected)
}

fn injected_index_failure(point: &str) -> Result<()> {
    #[cfg(debug_assertions)]
    if std::env::var("LOOM_TEST_PREPARED_INDEX_FAILURE_POINT")
        .ok()
        .is_some_and(|configured| configured.split(',').any(|item| item == point))
    {
        return Err(anyhow!("prepared index injected failure at {point}"));
    }
    Ok(())
}

fn injected_index_crash(point: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("LOOM_TEST_PREPARED_INDEX_CRASH_POINT").as_deref() == Ok(point) {
        std::process::exit(93);
    }
}

#[cfg(unix)]
fn publish_claimed_index(
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    _publish: &Path,
    index: &Path,
    expected: &[u8],
) -> Result<()> {
    capture_owned_lock(claim, capture, sentinel, lock, expected)?;
    injected_index_failure("after_lock_capture")?;
    injected_index_crash("after_lock_capture");
    crate::fs_util::rename_atomic(capture, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    injected_index_failure("after_index_rename")?;
    injected_index_crash("after_index_rename");
    remove_placeholder_lock(lock, sentinel)
}

#[cfg(windows)]
fn publish_claimed_index(
    claim: &Path,
    _capture: &Path,
    _sentinel: &Path,
    lock: &Path,
    publish: &Path,
    index: &Path,
    expected: &[u8],
) -> Result<()> {
    let exclusive = crate::fs_util::ExclusiveDeleteFile::open_owned(lock, claim, expected)?;
    injected_index_failure("after_lock_capture")?;
    injected_index_crash("after_lock_capture");
    fs::hard_link(claim, publish)?;
    crate::fs_util::sync_parent_directory(publish)?;
    crate::fs_util::rename_atomic(publish, index)?;
    crate::fs_util::sync_parent_directory(index)?;
    injected_index_failure("after_index_rename")?;
    injected_index_crash("after_index_rename");
    exclusive.delete()?;
    crate::fs_util::sync_parent_directory(lock)?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn publish_claimed_index(
    _claim: &Path,
    _capture: &Path,
    _sentinel: &Path,
    _lock: &Path,
    _publish: &Path,
    _index: &Path,
    _expected: &[u8],
) -> Result<()> {
    Err(anyhow!(
        "crash-safe Git index publication is unavailable on this platform"
    ))
}

#[cfg(unix)]
fn clear_completed_public_lock(
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    match read_regular_file(lock, "Git index lock")? {
        bytes if bytes == INDEX_LOCK_PLACEHOLDER => remove_placeholder_lock(lock, sentinel),
        _ if public_lock_is_owned(claim, lock, expected)? => {
            capture_owned_lock(claim, capture, sentinel, lock, expected)?;
            remove_private_entry(capture)?;
            crate::fs_util::sync_parent_directory(capture)?;
            remove_placeholder_lock(lock, sentinel)
        }
        _ => Err(anyhow!(
            "existing Git index lock is not owned by the completed transaction"
        )),
    }
}

#[cfg(windows)]
fn clear_completed_public_lock(
    claim: &Path,
    _capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    match read_regular_file(lock, "Git index lock")? {
        bytes if bytes == INDEX_LOCK_PLACEHOLDER => remove_placeholder_lock(lock, sentinel),
        _ => {
            let exclusive = crate::fs_util::ExclusiveDeleteFile::open_owned(lock, claim, expected)?;
            exclusive.delete()?;
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn clear_completed_public_lock(
    _claim: &Path,
    _capture: &Path,
    _sentinel: &Path,
    _lock: &Path,
    _expected: &[u8],
) -> Result<()> {
    Err(anyhow!(
        "crash-safe Git index lock cleanup is unavailable on this platform"
    ))
}

fn reconcile_legacy_sentinel(
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    if !path_entry_exists(sentinel)? {
        return Ok(());
    }
    let sentinel_bytes = read_regular_file(sentinel, "Git index lock sentinel")?;
    if sentinel_bytes == INDEX_LOCK_PLACEHOLDER {
        if path_entry_exists(lock)? && placeholder_is_owned(lock, sentinel)? {
            return Ok(());
        }
        if path_entry_exists(capture)? && placeholder_is_owned(capture, sentinel)? {
            remove_private_entry(capture)?;
            remove_private_entry(sentinel)?;
            crate::fs_util::sync_parent_directory(sentinel)?;
            return Ok(());
        }
        if !path_entry_exists(lock)? && !path_entry_exists(capture)? {
            remove_private_entry(sentinel)?;
            crate::fs_util::sync_parent_directory(sentinel)?;
            return Ok(());
        }
        if !path_entry_exists(capture)? && public_lock_is_owned(claim, lock, expected)? {
            remove_private_entry(sentinel)?;
            crate::fs_util::sync_parent_directory(sentinel)?;
            return Ok(());
        }
        return Err(anyhow!("unknown Git index lock placeholder marker state"));
    }
    if path_entry_exists(claim)?
        && !path_entry_exists(capture)?
        && captured_lock_is_owned(claim, sentinel, expected)?
    {
        require_owned_placeholder(lock, sentinel)?;
        crate::fs_util::rename_no_replace_atomic(sentinel, capture)?;
        crate::fs_util::sync_parent_directory(capture)?;
        return Ok(());
    }
    Err(anyhow!(
        "unknown Git index lock sentinel state was preserved for recovery"
    ))
}

#[cfg(unix)]
fn capture_owned_lock(
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    create_private_entry(sentinel, INDEX_LOCK_PLACEHOLDER)?;
    fs::hard_link(sentinel, capture)?;
    crate::fs_util::sync_parent_directory(capture)?;
    crate::fs_util::capture_with_placeholder_atomic(lock, capture)?;
    crate::fs_util::sync_parent_directory(lock)?;
    if captured_lock_is_owned(claim, capture, expected)? {
        require_owned_placeholder(lock, sentinel)?;
        return Ok(());
    }
    restore_foreign_capture(capture, sentinel, lock)?;
    Err(anyhow!(
        "existing Git index lock is not owned by the durable transaction claim"
    ))
}

fn reconcile_private_capture(
    claim: &Path,
    capture: &Path,
    sentinel: &Path,
    lock: &Path,
    expected: &[u8],
) -> Result<()> {
    if !path_entry_exists(capture)? {
        return Ok(());
    }
    if path_entry_exists(claim)? && captured_lock_is_owned(claim, capture, expected)? {
        return Err(anyhow!("captured transaction Git index requires recovery"));
    }
    let capture_bytes = read_regular_file(capture, "captured Git index lock")?;
    if capture_bytes == INDEX_LOCK_PLACEHOLDER {
        if placeholder_is_owned(capture, sentinel)? && public_lock_is_owned(claim, lock, expected)?
        {
            remove_private_entry(capture)?;
            remove_private_entry(sentinel)?;
            crate::fs_util::sync_parent_directory(capture)?;
            return Ok(());
        }
        return Err(anyhow!(
            "Git index lock placeholder has no matching owned public lock"
        ));
    }
    Err(anyhow!(
        "unknown captured Git index lock was preserved for recovery"
    ))
}

fn restore_foreign_capture(capture: &Path, sentinel: &Path, lock: &Path) -> Result<()> {
    require_owned_placeholder(lock, sentinel)?;
    crate::fs_util::restore_capture_atomic(lock, capture)?;
    crate::fs_util::sync_parent_directory(lock)?;
    require_regular_exact(
        capture,
        INDEX_LOCK_PLACEHOLDER,
        "Git index lock placeholder",
    )?;
    remove_private_entry(capture)?;
    remove_private_entry(sentinel)?;
    crate::fs_util::sync_parent_directory(capture)?;
    Ok(())
}

fn create_private_entry(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    crate::fs_util::sync_parent_directory(path)?;
    Ok(())
}

fn placeholder_is_owned(path: &Path, sentinel: &Path) -> Result<bool> {
    Ok(path_entry_exists(path)?
        && path_entry_exists(sentinel)?
        && crate::fs_util::same_file_identity_paths(path, sentinel)?
        && fs::read(path)? == INDEX_LOCK_PLACEHOLDER
        && fs::read(sentinel)? == INDEX_LOCK_PLACEHOLDER)
}

fn require_owned_placeholder(lock: &Path, sentinel: &Path) -> Result<()> {
    if !placeholder_is_owned(lock, sentinel)? {
        return Err(anyhow!(
            "Git index lock placeholder is not transaction-owned"
        ));
    }
    Ok(())
}

fn remove_placeholder_lock(lock: &Path, sentinel: &Path) -> Result<()> {
    require_owned_placeholder(lock, sentinel)?;
    fs::remove_file(lock)?;
    remove_private_entry(sentinel)?;
    crate::fs_util::sync_parent_directory(lock)?;
    Ok(())
}

fn captured_lock_is_owned(claim: &Path, capture: &Path, expected: &[u8]) -> Result<bool> {
    Ok(crate::fs_util::same_file_identity_paths(claim, capture)?
        && fs::read(claim)? == expected
        && fs::read(capture)? == expected)
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

pub(super) fn path_entry_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
