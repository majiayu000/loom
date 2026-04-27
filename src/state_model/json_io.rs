//! Filesystem helpers for V3 state JSON/JSONL persistence.
//!
//! These are generic over any serde-serializable type; they don't know
//! anything about the V3 schema itself. Schema-aware orchestration (load
//! order, version validation, snapshot assembly) lives in
//! [`super::persistence`].

use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(super) fn ensure_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if path.exists() {
        return Ok(());
    }
    write_json_file(path, value)
}

pub(super) fn ensure_text_file(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_atomic(path, contents)
}

pub(super) fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serialize_json_file(value)?;
    write_atomic(path, &raw)
}

pub(super) fn serialize_json_file<T: Serialize>(value: &T) -> Result<String> {
    let raw = serde_json::to_string_pretty(value).context("failed to encode v3 json")?;
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
    let parent = path
        .parent()
        .context("cannot append v3 jsonl file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    let raw = serde_json::to_string(value)
        .with_context(|| format!("failed to encode v3 jsonl line {}", path.display()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open v3 jsonl file {}", path.display()))?;
    writeln!(file, "{}", raw)
        .with_context(|| format!("failed to append v3 jsonl file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync v3 jsonl file {}", path.display()))?;
    Ok(())
}

pub(super) fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read v3 json file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse v3 json file {}", path.display()))
}

pub(super) fn read_json_lines<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open v3 jsonl file {}", path.display()))?;
    if file
        .metadata()
        .with_context(|| format!("failed to stat v3 jsonl file {}", path.display()))?
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
                "failed to parse line {} from v3 jsonl file {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(item);
    }
    Ok(items)
}

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("cannot write atomic v3 file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;

    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));

    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file {}", tmp_path.display()))?;
    }

    crate::fs_util::rename_atomic(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
}
