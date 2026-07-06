use std::fs;
use std::path::Path;

use crate::state_model::{
    RegistryBindingsFile, RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
    RegistryTargetsFile,
};
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{map_io, map_registry_state};
use super::resolve::{ActivationResolved, normalize_existing_or_raw};

pub(super) fn remove_safe_symlink_projection(
    skill_src: &Path,
    resolved: &ActivationResolved,
) -> std::result::Result<(), CommandFailure> {
    match fs::symlink_metadata(&resolved.materialized_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if !projection_path_is_safe_symlink(&resolved.materialized_path, skill_src) {
                return Err(CommandFailure::new(
                    ErrorCode::ProjectionConflict,
                    format!(
                        "projection path '{}' is a symlink but does not point at registry skill '{}'",
                        resolved.materialized_path.display(),
                        resolved.selection.skill
                    ),
                ));
            }
            remove_symlink(&resolved.materialized_path).map_err(map_io)?;
            Ok(())
        }
        Ok(_) => Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            format!(
                "deactivate refuses to delete non-symlink projection '{}'",
                resolved.materialized_path.display()
            ),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(map_io(err)),
    }
}

pub(super) fn save_activation_state(
    paths: &RegistryStatePaths,
    targets: &RegistryTargetsFile,
    bindings: &RegistryBindingsFile,
    rules: &RegistryRulesFile,
    projections: &RegistryProjectionsFile,
    original_targets: &RegistryTargetsFile,
) -> std::result::Result<(), CommandFailure> {
    paths.save_targets(targets).map_err(map_registry_state)?;
    if let Err(err) = paths.save_bindings_rules_projections(bindings, rules, projections) {
        let restore = paths.save_targets(original_targets);
        if let Err(restore_err) = restore {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!(
                    "failed to save activation state and failed to rollback targets: {}; rollback error: {}",
                    err, restore_err
                ),
            ));
        }
        return Err(map_registry_state(err));
    }
    Ok(())
}

pub(super) fn restore_activation_state(
    paths: &RegistryStatePaths,
    targets: &RegistryTargetsFile,
    bindings: &RegistryBindingsFile,
    rules: &RegistryRulesFile,
    projections: &RegistryProjectionsFile,
) -> std::result::Result<(), CommandFailure> {
    paths.save_targets(targets).map_err(map_registry_state)?;
    paths
        .save_bindings_rules_projections(bindings, rules, projections)
        .map_err(map_registry_state)?;
    Ok(())
}

fn projection_path_is_safe_symlink(path: &Path, skill_src: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(link_target) = fs::read_link(path) else {
        return false;
    };
    let expected = normalize_existing_or_raw(skill_src);
    let actual = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(link_target)
    };
    normalize_existing_or_raw(&actual) == expected
}

#[cfg(unix)]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path)
}
