use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use walkdir::WalkDir;

pub(super) fn ensure_projection_symlinks_contained(
    src: &Path,
    allow_dangling_in_tree: bool,
) -> Result<()> {
    let source_root = fs::canonicalize(src)
        .with_context(|| format!("failed to canonicalize projection source {}", src.display()))?;

    for entry in WalkDir::new(&source_root).follow_links(false).into_iter() {
        let entry =
            entry.with_context(|| format!("failed to walk projection source {}", src.display()))?;
        if !entry.file_type().is_symlink() {
            continue;
        }

        let rel = entry
            .path()
            .strip_prefix(&source_root)
            .unwrap_or(entry.path());
        let link_target = fs::read_link(entry.path())
            .with_context(|| format!("failed to read symlink {}", entry.path().display()))?;
        let resolved_target =
            resolve_symlink_target(entry.path(), &link_target, allow_dangling_in_tree)
                .with_context(|| {
                    format!(
                        "unsafe symlink '{}' has unresolved target '{}'",
                        rel.display(),
                        link_target.display()
                    )
                })?;

        if !resolved_target.starts_with(&source_root) {
            return Err(anyhow!(
                "unsafe symlink '{}' resolves outside source directory '{}' to '{}'",
                rel.display(),
                source_root.display(),
                resolved_target.display()
            ));
        }
    }

    Ok(())
}

fn resolve_symlink_target(
    link_path: &Path,
    link_target: &Path,
    allow_dangling_in_tree: bool,
) -> Result<PathBuf> {
    let target = if link_target.is_absolute() {
        link_target.to_path_buf()
    } else {
        let parent = link_path
            .parent()
            .context("symlink path has no parent directory")?;
        parent.join(link_target)
    };
    match fs::canonicalize(&target) {
        Ok(target) => Ok(target),
        Err(err) if allow_dangling_in_tree && err.kind() == std::io::ErrorKind::NotFound => {
            Ok(normalize_dangling_symlink_target(&target))
        }
        Err(err) => Err(err)
            .with_context(|| format!("failed to canonicalize symlink target {}", target.display())),
    }
}

fn normalize_dangling_symlink_target(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
