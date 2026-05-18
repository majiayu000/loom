use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::json;
use walkdir::WalkDir;

use crate::gitops;
use crate::state::remove_path_if_exists;
use crate::state_model::{
    RegistryBindingsFile, RegistryProjectionTarget,
    RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
};
use crate::types::ErrorCode;

use super::super::helpers::{RegistryAuditStateBackup, restore_registry_audit_state};
use super::super::{CommandFailure, helpers::map_git};

pub(super) fn maybe_skill_fault(tag: &str) -> std::result::Result<(), CommandFailure> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            format!("fault injected at {}", tag),
        ));
    }
    Ok(())
}

pub(super) fn rollback_registry_state(
    paths: &RegistryStatePaths,
    original_bindings: &RegistryBindingsFile,
    original_rules: &RegistryRulesFile,
    original_projections: &RegistryProjectionsFile,
) {
    let _ = paths.save_bindings_rules_projections(
        original_bindings,
        original_rules,
        original_projections,
    );
}

pub(super) fn has_skill_entrypoint(path: &Path) -> bool {
    path.join("SKILL.md").is_file() || path.join("skill.md").is_file()
}

pub(super) fn materialized_dirs_equal(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let left_files = collect_materialized_files(left)?;
    let right_files = collect_materialized_files(right)?;
    if left_files.len() != right_files.len() {
        return Ok(false);
    }

    for (rel, left_body) in left_files {
        match right_files.get(&rel) {
            Some(right_body) if right_body == &left_body => {}
            _ => return Ok(false),
        }
    }

    Ok(true)
}

pub(super) fn collect_materialized_files(root: &Path) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", root.display()))?;
        let rel = entry.path().strip_prefix(root).with_context(|| {
            format!(
                "failed to derive relative path for {} under {}",
                entry.path().display(),
                root.display()
            )
        })?;
        if rel.as_os_str().is_empty() || entry.file_type().is_dir() {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Ok(BTreeMap::from([(
                rel.to_path_buf(),
                b"__loom_symlink_marker__".to_vec(),
            )]));
        }
        if entry.file_type().is_file() {
            let body = fs::read(entry.path())
                .with_context(|| format!("failed to read {}", entry.path().display()))?;
            files.insert(rel.to_path_buf(), body);
        }
    }

    Ok(files)
}

pub(super) fn observed_import_targets(
    targets: &[RegistryProjectionTarget],
    target_id: Option<&str>,
) -> std::result::Result<Vec<RegistryProjectionTarget>, CommandFailure> {
    if let Some(target_id) = target_id {
        let target = targets
            .iter()
            .find(|target| target.target_id == target_id)
            .cloned()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::TargetNotFound,
                    format!("target '{}' not found", target_id),
                )
            })?;
        if target.ownership != "observed" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' has ownership '{}' and cannot be imported as observed",
                    target.target_id, target.ownership
                ),
            ));
        }
        return Ok(vec![target]);
    }

    Ok(targets
        .iter()
        .filter(|target| target.ownership == "observed")
        .cloned()
        .collect())
}

pub(super) fn observed_skill_copy_source(
    source_path: &Path,
    file_type: &fs::FileType,
    skipped: &mut Vec<serde_json::Value>,
    target: &RegistryProjectionTarget,
) -> Option<(PathBuf, &'static str, Option<String>)> {
    if file_type.is_dir() {
        return Some((source_path.to_path_buf(), "directory", None));
    }
    if !file_type.is_symlink() {
        return None;
    }

    let metadata = match fs::metadata(source_path) {
        Ok(metadata) => metadata,
        Err(err) => {
            skipped.push(json!({
                "target_id": target.target_id.clone(),
                "source": source_path.display().to_string(),
                "reason": "symlink-target-failed",
                "error": err.to_string(),
            }));
            return None;
        }
    };
    if !metadata.is_dir() {
        return None;
    }

    match fs::canonicalize(source_path) {
        Ok(resolved) => {
            let display = resolved.display().to_string();
            Some((resolved, "symlink", Some(display)))
        }
        Err(err) => {
            skipped.push(json!({
                "target_id": target.target_id.clone(),
                "source": source_path.display().to_string(),
                "reason": "symlink-resolve-failed",
                "error": err.to_string(),
            }));
            None
        }
    }
}

pub(super) fn reset_command_created_commits(ctx: &crate::state::AppContext, previous_head: &str) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "--soft", previous_head]);
}

pub(super) fn unstage_registry_state(ctx: &crate::state::AppContext) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/registry"]);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/v3"]);
}

pub(super) fn stage_registry_state(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
) -> std::result::Result<(), CommandFailure> {
    gitops::run_git(ctx, &["add", "-A", "--", "state/registry"]).map_err(map_git)?;
    let legacy_v3_tracked =
        gitops::run_git_allow_failure(ctx, &["ls-files", "--error-unmatch", "--", "state/v3"])
            .map_err(map_git)?
            .status
            .success();
    if paths.state_dir.join("v3").exists() || legacy_v3_tracked {
        gitops::run_git(ctx, &["add", "-A", "--", "state/v3"]).map_err(map_git)?;
    }
    Ok(())
}

pub(super) fn rollback_registry_audit_after_failure(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
    had_registry_layout: bool,
    had_legacy_layout: bool,
) {
    let _ = restore_registry_audit_state(paths, registry_backup);
    if !had_registry_layout && !had_legacy_layout {
        let _ = remove_path_if_exists(&paths.registry_dir);
    }
    unstage_registry_state(ctx);
}
