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
