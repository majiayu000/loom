use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::json;

use crate::state_model::RegistryProjectionTarget;
use crate::types::ErrorCode;

use super::CommandFailure;

#[derive(Debug, Clone)]
pub(crate) struct TargetRootInspection {
    pub(crate) stable: bool,
    pub(crate) kind: &'static str,
    pub(crate) resolved_path: Option<PathBuf>,
    pub(crate) error: Option<String>,
}

pub(crate) fn inspect_target_root(path: &Path) -> TargetRootInspection {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return TargetRootInspection {
                stable: false,
                kind: "missing",
                resolved_path: None,
                error: None,
            };
        }
        Err(error) => {
            return TargetRootInspection {
                stable: false,
                kind: "unreadable",
                resolved_path: None,
                error: Some(error.to_string()),
            };
        }
    };

    let kind = if metadata.file_type().is_symlink() {
        "symlink"
    } else if metadata.is_dir() {
        "directory"
    } else {
        "other"
    };
    match path.canonicalize() {
        Ok(resolved_path) => TargetRootInspection {
            stable: kind == "directory" && resolved_path == path,
            kind,
            resolved_path: Some(resolved_path),
            error: None,
        },
        Err(error) => TargetRootInspection {
            stable: false,
            kind,
            resolved_path: None,
            error: Some(error.to_string()),
        },
    }
}

pub(crate) fn target_paths_equivalent(stored_path: &str, candidate: &Path) -> bool {
    normalize_existing_or_missing(Path::new(stored_path))
        == normalize_existing_or_missing(candidate)
}

pub(crate) fn normalize_existing_or_missing(path: &Path) -> PathBuf {
    let normalized = normalize_path(path);
    if let Ok(canonical) = fs::canonicalize(&normalized) {
        return normalize_path(&canonical);
    }

    let mut probe = normalized.clone();
    let mut suffix = Vec::new();
    while !probe.exists() {
        let Some(name) = probe.file_name().map(|name| name.to_os_string()) else {
            break;
        };
        suffix.push(name);
        if !probe.pop() {
            break;
        }
    }
    if probe.exists()
        && let Ok(mut canonical) = fs::canonicalize(&probe)
    {
        for component in suffix.iter().rev() {
            canonical.push(component);
        }
        return normalize_path(&canonical);
    }

    normalized
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

pub(crate) fn managed_target_alias<'a>(
    targets: &'a [RegistryProjectionTarget],
    candidate: &Path,
    excluding_target_id: Option<&str>,
) -> Option<&'a RegistryProjectionTarget> {
    targets.iter().find(|target| {
        target.ownership == crate::core::vocab::Ownership::Managed
            && excluding_target_id != Some(target.target_id.as_str())
            && target_paths_equivalent(&target.path, candidate)
    })
}

pub(crate) fn ensure_managed_target_root_is_unique(
    targets: &[RegistryProjectionTarget],
    candidate: &Path,
    excluding_target_id: Option<&str>,
) -> std::result::Result<(), CommandFailure> {
    let Some(existing) = managed_target_alias(targets, candidate, excluding_target_id) else {
        return Ok(());
    };
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!(
            "target path '{}' is already managed by target '{}' for agent '{}'; one physical directory cannot have multiple managed target owners",
            candidate.display(),
            existing.target_id,
            existing.agent
        ),
    );
    failure.details = json!({
        "path": candidate.display().to_string(),
        "conflicting_target_id": existing.target_id,
        "conflicting_agent": existing.agent
    });
    Err(failure)
}

pub(crate) fn ensure_managed_target_root_is_stable(
    target: &RegistryProjectionTarget,
) -> std::result::Result<(), CommandFailure> {
    if target.ownership != crate::core::vocab::Ownership::Managed {
        return Ok(());
    }
    let inspection = inspect_target_root(Path::new(&target.path));
    if inspection.stable {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!(
            "managed target '{}' root '{}' no longer matches its registered physical directory",
            target.target_id, target.path
        ),
    );
    failure.details = json!({
        "target_id": target.target_id,
        "agent": target.agent,
        "registered_path": target.path,
        "root_kind": inspection.kind,
        "resolved_path": inspection.resolved_path.map(|path| path.display().to_string()),
        "inspection_error": inspection.error
    });
    Err(failure)
}
