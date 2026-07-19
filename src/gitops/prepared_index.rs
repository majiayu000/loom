use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::sha256::{Sha256, to_hex};

use super::{
    AppContext, path_exists_or_is_tracked, resolve_git_index_path, run_git,
    run_git_allow_failure_in_with_env, run_git_in_with_env,
};

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
    let guarded = prepared_index_aux_path(ctx, prepared_index, ".lock-guard")?;
    let publish = prepared_index_aux_path(ctx, prepared_index, ".lock-publish")?;

    if recovery
        && !path_entry_exists(&lock)?
        && !path_entry_exists(&claim)?
        && !path_entry_exists(&capture)?
    {
        return Ok(false);
    }
    let prepared_bytes = read_regular_file(prepared_index, "prepared Git index")?;
    crate::fs_util::sync_file_and_parent(prepared_index)?;
    reconcile_private_capture(&claim, &capture, &lock, &prepared_bytes)?;
    remove_private_entry(&guarded)?;
    remove_private_entry(&publish)?;

    if recovery {
        match finish_completed_index_install(&index, &claim, &capture, &lock, &prepared_bytes) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => return Err(retained_lock_error(&lock, error)),
        }
    }

    if recovery && !path_entry_exists(&claim)? {
        return Err(anyhow!(
            "existing Git index lock has no durable transaction claim"
        ));
    }

    create_or_validate_claim(&claim, &prepared_bytes)?;
    crate::fs_util::write_atomic_bytes(prepared_index, &prepared_bytes)?;
    require_regular_exact(prepared_index, &prepared_bytes, "prepared Git index")?;
    if let Err(error) = reserve_public_lock(&claim, &capture, &lock, &prepared_bytes) {
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
        capture_owned_lock(&claim, &capture, &lock, &prepared_bytes)?;
        injected_index_failure("after_lock_capture")?;
        injected_index_crash("after_lock_capture");
        crate::fs_util::rename_atomic(&capture, &index)?;
        crate::fs_util::sync_parent_directory(&index)?;
        injected_index_failure("after_index_rename")?;
        injected_index_crash("after_index_rename");
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
    lock: &Path,
    expected: &[u8],
) -> Result<bool> {
    if !path_entry_exists(index)? || !path_entry_exists(claim)? {
        return Ok(false);
    }
    let index_metadata = fs::symlink_metadata(index)?;
    let claim_metadata = fs::symlink_metadata(claim)?;
    if !index_metadata.file_type().is_file()
        || !claim_metadata.file_type().is_file()
        || !same_file_identity(&index_metadata, &claim_metadata)
        || fs::read(index)? != expected
        || fs::read(claim)? != expected
    {
        return Ok(false);
    }
    if path_entry_exists(lock)? {
        capture_and_remove_owned_lock(claim, capture, lock, expected)?;
    }
    remove_private_entry(capture)?;
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

fn reserve_public_lock(claim: &Path, capture: &Path, lock: &Path, expected: &[u8]) -> Result<()> {
    match fs::hard_link(claim, lock) {
        Ok(()) => {
            injected_index_failure("after_lock_link")?;
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            capture_and_remove_owned_lock(claim, capture, lock, expected)?;
            fs::hard_link(claim, lock)?;
            injected_index_failure("after_lock_link")?;
            crate::fs_util::sync_parent_directory(lock)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn public_lock_is_owned(claim: &Path, lock: &Path, expected: &[u8]) -> Result<bool> {
    if !path_entry_exists(claim)? || !path_entry_exists(lock)? {
        return Ok(false);
    }
    let claim_metadata = fs::symlink_metadata(claim)?;
    let lock_metadata = fs::symlink_metadata(lock)?;
    Ok(claim_metadata.file_type().is_file()
        && lock_metadata.file_type().is_file()
        && same_file_identity(&claim_metadata, &lock_metadata)
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

pub(super) fn prepared_index_aux_path(
    ctx: &AppContext,
    prepared_index: &Path,
    suffix: &str,
) -> Result<PathBuf> {
    let index = resolve_git_index_path(ctx, &[])?;
    let parent = index
        .parent()
        .ok_or_else(|| anyhow!("Git index has no parent: {}", index.display()))?;
    let identity = prepared_index_identity(prepared_index);
    Ok(parent.join(format!(".loom-index-{}{suffix}", &identity[..32])))
}

pub fn prepared_index_claim_exists(ctx: &AppContext, prepared_index: &Path) -> Result<bool> {
    path_entry_exists(&prepared_index_aux_path(
        ctx,
        prepared_index,
        ".lock-claim",
    )?)
}

fn prepared_index_identity(path: &Path) -> String {
    let mut hasher = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            hasher.update(&unit.to_le_bytes());
        }
    }
    #[cfg(not(any(unix, windows)))]
    hasher.update(path.to_string_lossy().as_bytes());
    to_hex(&hasher.finalize())
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
