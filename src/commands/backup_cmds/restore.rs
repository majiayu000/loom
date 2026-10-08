use std::fs;
use std::io;
use std::path::Path;

use crate::commands::CommandFailure;
use crate::commands::helpers::{map_arg, map_io};
use crate::fs_util::rename_no_replace_atomic;
use crate::state::AppContext;
use crate::types::ErrorCode;

pub(super) fn validate_root(
    root: &Path,
    _force_empty_root: bool,
) -> std::result::Result<bool, CommandFailure> {
    AppContext::new(Some(root.to_path_buf()))
        .map_err(map_io)?
        .ensure_not_loom_tool_repo_root()
        .map_err(map_arg)?;
    match fs::symlink_metadata(root) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(map_io(err)),
        Ok(_) => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("restore root must not exist: {}", root.display()),
        )),
    }
}

pub(super) fn activate(
    staging: &Path,
    root: &Path,
    _destination_existed: bool,
    _force_empty_root: bool,
) -> std::result::Result<(), CommandFailure> {
    test_pause("before_activation", staging)?;
    test_pause("before_rename", staging)?;
    rename_no_replace_atomic(staging, root).map_err(map_io)
}

#[cfg(debug_assertions)]
pub(super) fn test_pause(point: &str, staging: &Path) -> std::result::Result<(), CommandFailure> {
    if std::env::var("LOOM_TEST_BACKUP_RESTORE_PAUSE_POINT")
        .ok()
        .as_deref()
        != Some(point)
    {
        return Ok(());
    }
    let directory = std::env::var_os("LOOM_TEST_BACKUP_RESTORE_PAUSE_DIR")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::IoError,
                "backup restore pause directory is absent",
            )
        })?;
    fs::create_dir_all(&directory).map_err(map_io)?;
    fs::write(
        directory.join("ready"),
        staging.to_string_lossy().as_bytes(),
    )
    .map_err(map_io)?;
    for _ in 0..2_000 {
        if directory.join("release").try_exists().map_err(map_io)? {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Err(CommandFailure::new(
        ErrorCode::IoError,
        "backup restore test pause timed out",
    ))
}

#[cfg(not(debug_assertions))]
pub(super) fn test_pause(_point: &str, _staging: &Path) -> std::result::Result<(), CommandFailure> {
    Ok(())
}
