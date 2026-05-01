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

pub(crate) fn restore_path_from_backup(path: &Path, backup: &serde_json::Value) -> Result<()> {
    let backup_path = backup
        .get("backup_path")
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
        .ok_or_else(|| anyhow!("backup record missing backup_path"))?;
    let kind = backup
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("backup record missing kind"))?;

    remove_path_if_exists(path)?;
    match kind {
        "dir" => copy_dir_recursive(backup_path, path),
        "file" => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create restore parent {}", parent.display())
                })?;
            }
            fs::copy(backup_path, path).with_context(|| {
                format!(
                    "failed to restore file backup {} to {}",
                    backup_path.display(),
                    path.display()
                )
            })?;
            Ok(())
        }
        "symlink" => restore_symlink_backup(backup_path, path),
        other => Err(anyhow!("unsupported backup kind '{}'", other)),
    }
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

fn restore_symlink_backup(backup_path: &Path, dst: &Path) -> Result<()> {
    let raw = fs::read_to_string(backup_path.join("symlink.json")).with_context(|| {
        format!(
            "failed to read symlink backup metadata under {}",
            backup_path.display()
        )
    })?;
    let payload: serde_json::Value =
        serde_json::from_str(&raw).context("failed to parse symlink backup metadata")?;
    let target = payload
        .get("target")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("symlink backup missing target"))?;
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create restore parent {}", parent.display()))?;
    }
    create_symlink_dir(Path::new(target), dst)
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

#[cfg(test)]
mod tests {
    use super::copy_dir_recursive_without_symlinks;
    use std::fs;

    #[cfg(unix)]
    #[test]
    fn copy_without_symlinks_rejects_nested_symlink() {
        let base = std::env::temp_dir().join(format!(
            "loom-copy-no-symlink-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let src = base.join("src");
        let dst = base.join("dst");
        fs::create_dir_all(&src).expect("src dir");
        fs::write(src.join("SKILL.md"), "# skill\n").expect("skill file");
        std::os::unix::fs::symlink("/tmp/secret", src.join("secret-link")).expect("symlink");

        let err =
            copy_dir_recursive_without_symlinks(&src, &dst).expect_err("symlink must be rejected");
        assert!(
            err.to_string().contains("unsupported symlink"),
            "unexpected error: {err}"
        );
        assert!(
            !dst.join("secret-link").exists(),
            "symlink path must not be copied"
        );

        let _ = fs::remove_dir_all(base);
    }
}
