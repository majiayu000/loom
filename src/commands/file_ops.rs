use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::gitops;
use crate::state::{AppContext, remove_path_if_exists};

use super::helpers::slugify;

// ---------------------------------------------------------------------------
// Git field reader
// ---------------------------------------------------------------------------

pub(crate) fn read_git_field(
    ctx: &AppContext,
    args: &[&str],
    warnings: &mut Vec<String>,
) -> Option<String> {
    match gitops::run_git(ctx, args) {
        Ok(value) if value.is_empty() => None,
        Ok(value) => Some(value),
        Err(err) => {
            warnings.push(format!("git {:?} unavailable: {}", args, err));
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Rollback / backup / copy helpers
// ---------------------------------------------------------------------------

pub(crate) fn rollback_added_skill(ctx: &AppContext, skill_rel: &str, dst: &Path) {
    let _ = remove_path_if_exists(dst);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", skill_rel]);
}

pub(crate) fn backup_path_if_exists(
    ctx: &AppContext,
    path: &Path,
    reason: &str,
) -> Result<Option<serde_json::Value>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to inspect path before backup: {}", path.display())
            });
        }
    };

    let ts = Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
    let entry = format!(
        "{}-{}-{}",
        slugify(reason),
        slugify(
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("path")
        ),
        Uuid::new_v4().simple()
    );
    let backup_root = ctx.state_dir.join("backups").join(ts);
    fs::create_dir_all(&backup_root)
        .with_context(|| format!("failed to create backup root {}", backup_root.display()))?;
    let backup_path = backup_root.join(entry);

    let kind = if metadata.file_type().is_symlink() {
        backup_symlink_metadata(path, &backup_path)?;
        "symlink"
    } else if metadata.is_dir() {
        copy_dir_recursive(path, &backup_path)?;
        "dir"
    } else {
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create backup parent {}", parent.display()))?;
        }
        fs::copy(path, &backup_path).with_context(|| {
            format!(
                "failed to copy file {} to {}",
                path.display(),
                backup_path.display()
            )
        })?;
        "file"
    };

    Ok(Some(json!({
        "reason": reason,
        "kind": kind,
        "original_path": path.display().to_string(),
        "backup_path": backup_path.display().to_string()
    })))
}

fn backup_symlink_metadata(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .with_context(|| format!("failed to create symlink backup dir {}", dst.display()))?;
    let target = fs::read_link(src)
        .with_context(|| format!("failed to resolve symlink {}", src.display()))?;

    let payload = json!({
        "source": src.display().to_string(),
        "target": target.display().to_string()
    });
    let raw = serde_json::to_string_pretty(&payload)?;
    fs::write(dst.join("symlink.json"), raw + "\n").with_context(|| {
        format!(
            "failed to write symlink backup metadata for {}",
            src.display()
        )
    })?;
    Ok(())
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("source does not exist: {}", src.display()));
    }
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in WalkDir::new(src).follow_links(true).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", src.display()))?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

pub(crate) fn copy_dir_recursive_without_symlinks(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("source does not exist: {}", src.display()));
    }
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;

    for entry in WalkDir::new(src).follow_links(false).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", src.display()))?;
        let rel = entry.path().strip_prefix(src)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Err(anyhow!(
                "source contains unsupported symlink entry '{}'",
                rel.display()
            ));
        }

        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

#[cfg(unix)]
pub(super) fn create_symlink_dir(src: &Path, dst: &Path) -> Result<()> {
    std::os::unix::fs::symlink(src, dst).context("failed to create symlink")?;
    Ok(())
}

#[cfg(windows)]
pub(super) fn create_symlink_dir(src: &Path, dst: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(src, dst).context("failed to create symlink")?;
    Ok(())
}
