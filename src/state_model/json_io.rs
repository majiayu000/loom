//! Filesystem helpers for Registry state JSON/JSONL persistence.
//!
//! These are generic over any serde-serializable type; they don't know
//! anything about the Registry schema itself. Schema-aware orchestration (load
//! order, version validation, snapshot assembly) lives in
//! [`super::persistence`].

use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
use crate::fs_util::exchange_paths_atomic;
use crate::fs_util::{append_jsonl_raw, write_atomic};

pub(super) fn ensure_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if path.exists() {
        ensure_existing_file(path)?;
        return Ok(());
    }
    write_json_file(path, value)
}

pub(super) fn ensure_text_file(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        ensure_existing_file(path)?;
        return Ok(());
    }
    Ok(write_atomic(path, contents)?)
}

fn ensure_existing_file(path: &Path) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    Err(anyhow!(
        "registry path exists but is not a file: {}",
        path.display()
    ))
}

pub(super) fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serialize_json_file(value)?;
    Ok(write_atomic(path, &raw)?)
}

/// Atomically replace a JSON file only when the displaced value still matches
/// the caller's reviewed baseline.
pub(super) fn compare_exchange_json_file<T>(
    path: &Path,
    expected: &T,
    replacement: &T,
) -> Result<bool>
where
    T: Serialize,
{
    let expected = serde_json::to_value(expected).context("failed to encode expected json")?;
    let raw = serialize_json_file(replacement)?;
    let parent = path
        .parent()
        .context("cannot replace json file without parent")?;
    fs::create_dir_all(parent)?;
    let candidate = parent.join(format!(
        ".{}.cas-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&candidate)
        .with_context(|| format!("failed to create json candidate {}", candidate.display()))?;
    file.write_all(raw.as_bytes())?;
    file.sync_all()?;
    drop(file);

    compare_exchange_json_candidate(path, &candidate, &expected)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn compare_exchange_json_candidate(
    path: &Path,
    candidate: &Path,
    expected: &Value,
) -> Result<bool> {
    if let Err(error) = exchange_paths_atomic(candidate, path) {
        let cleanup = fs::remove_file(candidate)
            .err()
            .map_or_else(|| "succeeded".to_string(), |error| error.to_string());
        return Err(anyhow!(
            "failed to atomically exchange json file {}: {}; candidate cleanup: {}",
            path.display(),
            error,
            cleanup
        ));
    }
    let matches = fs::read(candidate)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .as_ref()
        == Some(expected);
    if !matches {
        exchange_paths_atomic(candidate, path).with_context(|| {
            format!("failed to restore mismatched json file {}", path.display())
        })?;
        fs::remove_file(candidate).with_context(|| {
            format!(
                "failed to remove rejected json candidate {}",
                candidate.display()
            )
        })?;
        return Ok(false);
    }
    if let Err(cleanup) = fs::remove_file(candidate) {
        let restore = exchange_paths_atomic(candidate, path);
        let rejected_cleanup = if restore.is_ok() {
            fs::remove_file(candidate)
                .err()
                .map_or_else(|| "succeeded".to_string(), |error| error.to_string())
        } else {
            "retained displaced original after restoration failure".to_string()
        };
        return Err(anyhow!(
            "failed to retire displaced json file {}: {}; restoration: {}; candidate cleanup: {}",
            candidate.display(),
            cleanup,
            restore
                .map(|_| "succeeded".to_string())
                .unwrap_or_else(|error| error.to_string()),
            rejected_cleanup
        ));
    }
    Ok(true)
}

#[cfg(windows)]
fn compare_exchange_json_candidate(
    path: &Path,
    candidate: &Path,
    expected: &Value,
) -> Result<bool> {
    let backup = path.with_extension(format!("cas-backup-{}", uuid::Uuid::new_v4()));
    replace_file_with_backup_windows(path, candidate, &backup)?;
    let matches = fs::read(&backup)
        .ok()
        .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .as_ref()
        == Some(expected);
    if !matches {
        replace_file_with_backup_windows(path, &backup, candidate)?;
        fs::remove_file(candidate).with_context(|| {
            format!(
                "failed to remove rejected json candidate {}",
                candidate.display()
            )
        })?;
        return Ok(false);
    }
    fs::remove_file(&backup)
        .with_context(|| format!("failed to retire displaced json file {}", backup.display()))?;
    Ok(true)
}

#[cfg(windows)]
fn replace_file_with_backup_windows(path: &Path, replacement: &Path, backup: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const REPLACEFILE_WRITE_THROUGH: u32 = 0x1;
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let wide = |value: &Path| {
        value
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let path = wide(path);
    let replacement = wide(replacement);
    let backup = wide(backup);
    let ok = unsafe {
        ReplaceFileW(
            path.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error())
            .context("atomic Windows file replacement failed");
    }
    Ok(())
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    windows
)))]
fn compare_exchange_json_candidate(
    _path: &Path,
    candidate: &Path,
    _expected: &Value,
) -> Result<bool> {
    let cleanup = fs::remove_file(candidate)
        .err()
        .map_or_else(|| "succeeded".to_string(), |error| error.to_string());
    Err(anyhow!(
        "atomic JSON compare-and-exchange is unsupported; candidate cleanup: {}",
        cleanup
    ))
}

pub(super) fn serialize_json_file<T: Serialize>(value: &T) -> Result<String> {
    let raw = serde_json::to_string_pretty(value).context("failed to encode registry json")?;
    Ok(raw + "\n")
}

/// Two-phase batch atomic write: write all temp files, then rename them all.
/// Minimizes the crash window for multi-file state updates. On a rename
/// failure midway, any temp files not yet renamed are cleaned up.
pub(super) fn write_atomic_batch(files: &[(&Path, &str)]) -> Result<()> {
    let mut staged: Vec<(PathBuf, &Path)> = Vec::with_capacity(files.len());

    // Phase 1: write all temp files
    for &(target, contents) in files {
        let parent = target
            .parent()
            .context("cannot write batch file without parent")?;
        fs::create_dir_all(parent)?;
        let tmp_path = parent.join(format!(
            ".{}.tmp-{}",
            target.file_name().unwrap_or_default().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        staged.push((tmp_path, target));
    }

    // Phase 2: rename all (minimal crash window)
    for (tmp, target) in &staged {
        if let Err(err) = crate::fs_util::rename_atomic(tmp, target) {
            for (remaining, _) in &staged {
                let _ = fs::remove_file(remaining);
            }
            return Err(err)
                .with_context(|| format!("batch rename failed for {}", target.display()));
        }
    }

    Ok(())
}

pub(super) fn append_json_line<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serde_json::to_string(value)
        .with_context(|| format!("failed to encode registry jsonl line {}", path.display()))?;
    append_jsonl_raw(path, &raw)
        .with_context(|| format!("failed to append registry jsonl file {}", path.display()))
}

pub(super) fn write_json_lines<T>(path: &Path, values: &[T]) -> Result<()>
where
    T: Serialize,
{
    let mut raw = String::new();
    for value in values {
        raw.push_str(
            &serde_json::to_string(value).with_context(|| {
                format!("failed to encode registry jsonl line {}", path.display())
            })?,
        );
        raw.push('\n');
    }
    Ok(write_atomic(path, &raw)?)
}

pub(super) fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read registry json file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse registry json file {}", path.display()))
}

pub(super) fn read_json_lines<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open registry jsonl file {}", path.display()))?;
    if file
        .metadata()
        .with_context(|| format!("failed to stat registry jsonl file {}", path.display()))?
        .len()
        == 0
    {
        return Ok(Vec::new());
    }
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read line {} from {}", index + 1, path.display())
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed to parse line {} from registry jsonl file {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_compare_exchange_installs_only_over_the_reviewed_value() {
        let root = std::env::temp_dir().join(format!("loom-json-cas-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create CAS fixture");
        let path = root.join("state.json");
        let reviewed = json!({"value": "reviewed"});
        let replacement = json!({"value": "replacement"});
        let external = json!({"value": "external"});
        write_json_file(&path, &reviewed).expect("write reviewed value");

        assert!(compare_exchange_json_file(&path, &reviewed, &replacement).expect("matching CAS"));
        assert_eq!(read_json_file::<Value>(&path).unwrap(), replacement);

        write_json_file(&path, &external).expect("write external value");
        assert!(
            !compare_exchange_json_file(&path, &reviewed, &replacement).expect("mismatching CAS")
        );
        assert_eq!(read_json_file::<Value>(&path).unwrap(), external);
        fs::remove_dir_all(root).expect("remove CAS fixture");
    }
}
