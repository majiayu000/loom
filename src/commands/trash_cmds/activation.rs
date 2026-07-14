use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::core::vocab::{Ownership, ProjectionMethod};
use crate::fs_util::{remove_path_if_exists, remove_symlink};
use crate::state_model::{
    RegistryBindingsFile, RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
};

use super::super::CommandFailure;
use super::super::codex_visibility::{normalize_existing_or_raw, projection_path_is_safe_symlink};
use super::super::file_ops::create_symlink_dir;
use super::super::helpers::{map_io, map_registry_state};
use super::super::skill_cmds::shared::{push_rollback_error, rollback_fault_active};
use crate::types::ErrorCode;

#[derive(Debug, Clone, Serialize)]
pub(super) struct TrashActivationImpact {
    pub(super) removed_rule_count: usize,
    pub(super) removed_projection_ids: Vec<String>,
    pub(super) links: Vec<TrashLinkImpact>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TrashLinkImpact {
    pub(super) path: String,
    pub(super) action: &'static str,
    pub(super) reason: &'static str,
}

pub(super) struct TrashActivationPlan {
    pub(super) impact: TrashActivationImpact,
    original_bindings: Option<RegistryBindingsFile>,
    original_rules: Option<RegistryRulesFile>,
    original_projections: Option<RegistryProjectionsFile>,
    next_rules: Option<RegistryRulesFile>,
    next_projections: Option<RegistryProjectionsFile>,
    deletable_links: Vec<PathBuf>,
    skill_path: PathBuf,
}

pub(super) struct AppliedTrashActivation {
    removed_links: Vec<RemovedLink>,
    registry_changed: bool,
}

struct RemovedLink {
    path: PathBuf,
    target: PathBuf,
}

impl TrashActivationPlan {
    pub(super) fn impact_json(&self) -> Value {
        json!(self.impact)
    }
}

pub(super) fn plan_trash_activation(
    paths: &RegistryStatePaths,
    skill_path: &Path,
    skill: &str,
) -> std::result::Result<TrashActivationPlan, CommandFailure> {
    let snapshot = if paths.registry_dir.exists() {
        paths.load_snapshot().map_err(map_registry_state)?
    } else if paths.legacy_state_dir_exists() {
        paths
            .legacy_state_paths()
            .load_snapshot()
            .map_err(map_registry_state)?
    } else {
        return Ok(empty_plan());
    };

    let removed_rule_count = snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.skill_id == skill)
        .count();
    let removed_projection_ids = snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill)
        .map(|projection| projection.instance_id.clone())
        .collect::<Vec<_>>();

    let mut deletable_links = Vec::new();
    let mut seen_paths = BTreeSet::new();
    let mut links = Vec::new();
    for projection in snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill)
    {
        let path = PathBuf::from(&projection.materialized_path);
        let normalized_path = normalize_projection_path(&path);
        let path_string = normalized_path.display().to_string();
        if !seen_paths.insert(path_string.clone()) {
            continue;
        }
        let target = snapshot
            .targets
            .targets
            .iter()
            .find(|target| target.target_id == projection.target_id);
        let (action, reason) = classify_link(
            target.map(|target| (Path::new(&target.path), target.ownership)),
            &normalized_path,
            skill_path,
            skill,
            projection.method,
        )?;
        if action == "delete" {
            deletable_links.push(normalized_path);
        }
        links.push(TrashLinkImpact {
            path: path_string,
            action,
            reason,
        });
    }

    let mut next_rules = snapshot.rules.clone();
    next_rules.rules.retain(|rule| rule.skill_id != skill);
    let mut next_projections = snapshot.projections.clone();
    next_projections
        .projections
        .retain(|projection| projection.skill_id != skill);

    Ok(TrashActivationPlan {
        impact: TrashActivationImpact {
            removed_rule_count,
            removed_projection_ids,
            links,
        },
        original_bindings: Some(snapshot.bindings),
        original_rules: Some(snapshot.rules),
        original_projections: Some(snapshot.projections),
        next_rules: Some(next_rules),
        next_projections: Some(next_projections),
        deletable_links,
        skill_path: skill_path.to_path_buf(),
    })
}

fn normalize_projection_path(path: &Path) -> PathBuf {
    let Some(file_name) = path.file_name() else {
        return normalize_existing_or_raw(path);
    };
    path.parent()
        .map(normalize_existing_or_raw)
        .unwrap_or_default()
        .join(file_name)
}

fn classify_link(
    target: Option<(&Path, Ownership)>,
    path: &Path,
    skill_path: &Path,
    skill: &str,
    method: ProjectionMethod,
) -> std::result::Result<(&'static str, &'static str), CommandFailure> {
    if method != ProjectionMethod::Symlink {
        return Ok(("retain", "non_symlink_projection"));
    }
    let Some((target_path, ownership)) = target else {
        return Ok(("retain", "target_not_registered"));
    };
    if ownership != Ownership::Managed {
        return Ok(("retain", "target_not_managed"));
    }
    if !is_expected_target_path(target_path, path, skill) {
        return Ok(("retain", "unexpected_materialized_path"));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_symlink() => Ok(("retain", "not_symlink")),
        Ok(_) if projection_path_is_safe_symlink(path, skill_path) => {
            Ok(("delete", "loom_managed_symlink"))
        }
        Ok(_) => Ok(("retain", "symlink_target_mismatch")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(("missing", "path_missing")),
        Err(err) => Err(map_io(err)),
    }
}

fn is_expected_target_path(target_path: &Path, materialized_path: &Path, skill: &str) -> bool {
    materialized_path.file_name().and_then(|name| name.to_str()) == Some(skill)
        && materialized_path.parent().is_some_and(|parent| {
            normalize_existing_or_raw(parent) == normalize_existing_or_raw(target_path)
        })
}

pub(super) fn apply_trash_activation(
    paths: &RegistryStatePaths,
    plan: &TrashActivationPlan,
) -> std::result::Result<AppliedTrashActivation, CommandFailure> {
    let mut removed_links = Vec::new();
    for path in &plan.deletable_links {
        if !projection_path_is_safe_symlink(path, &plan.skill_path) {
            let failure = CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "trash refuses to delete projection '{}' because it no longer points at registry skill '{}'",
                    path.display(),
                    plan.skill_path.display()
                ),
            );
            return Err(failure.with_rollback_errors(restore_removed_links(&removed_links)));
        }
        let target = match fs::read_link(path) {
            Ok(target) => target,
            Err(err) => {
                return Err(map_io(err).with_rollback_errors(restore_removed_links(&removed_links)));
            }
        };
        if let Err(err) = remove_symlink(path) {
            let rollback_errors = restore_removed_links(&removed_links);
            return Err(map_io(err).with_rollback_errors(rollback_errors));
        }
        removed_links.push(RemovedLink {
            path: path.clone(),
            target,
        });
    }

    let Some(bindings) = plan.original_bindings.as_ref() else {
        return Ok(AppliedTrashActivation {
            removed_links,
            registry_changed: false,
        });
    };
    let registry_changed =
        plan.impact.removed_rule_count > 0 || !plan.impact.removed_projection_ids.is_empty();
    if registry_changed
        && let Err(err) = paths.save_bindings_rules_projections(
            bindings,
            plan.next_rules.as_ref().expect("planned rules"),
            plan.next_projections.as_ref().expect("planned projections"),
        )
    {
        let mut rollback_errors = rollback_registry(paths, plan);
        rollback_errors.extend(restore_removed_links(&removed_links));
        return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
    }

    Ok(AppliedTrashActivation {
        removed_links,
        registry_changed,
    })
}

pub(super) fn rollback_trash_activation(
    paths: &RegistryStatePaths,
    plan: &TrashActivationPlan,
    applied: &AppliedTrashActivation,
) -> Vec<Value> {
    let mut errors = if applied.registry_changed {
        rollback_registry(paths, plan)
    } else {
        Vec::new()
    };
    errors.extend(restore_removed_links(&applied.removed_links));
    errors
}

pub(super) fn rollback_trash_source_and_activation(
    paths: &RegistryStatePaths,
    plan: &TrashActivationPlan,
    applied: &AppliedTrashActivation,
    skill_path: &Path,
    trash_skill_path: &Path,
    entry_path: &Path,
) -> Vec<Value> {
    let mut errors = Vec::new();
    let mut payload_restored = true;
    if fs::symlink_metadata(trash_skill_path).is_ok() {
        if rollback_fault_active("restore_source_path") {
            push_rollback_error(
                &mut errors,
                "restore_source_path",
                "fault injected at restore_source_path",
            );
            payload_restored = false;
        } else {
            if let Some(parent) = skill_path.parent()
                && let Err(err) = fs::create_dir_all(parent)
            {
                push_rollback_error(&mut errors, "restore_source_path", err);
                payload_restored = false;
            }
            if payload_restored && let Err(err) = fs::rename(trash_skill_path, skill_path) {
                push_rollback_error(&mut errors, "restore_source_path", err);
                payload_restored = false;
            }
        }
    }
    if payload_restored && let Err(err) = remove_path_if_exists(entry_path) {
        push_rollback_error(&mut errors, "remove_trash_entry", err);
    }
    errors.extend(rollback_trash_activation(paths, plan, applied));
    errors
}

fn rollback_registry(paths: &RegistryStatePaths, plan: &TrashActivationPlan) -> Vec<Value> {
    let mut errors = Vec::new();
    if rollback_fault_active("restore_registry_state") {
        push_rollback_error(
            &mut errors,
            "restore_registry_state",
            "fault injected at restore_registry_state",
        );
        return errors;
    }
    if let (Some(bindings), Some(rules), Some(projections)) = (
        plan.original_bindings.as_ref(),
        plan.original_rules.as_ref(),
        plan.original_projections.as_ref(),
    ) && let Err(err) = paths.save_bindings_rules_projections(bindings, rules, projections)
    {
        push_rollback_error(&mut errors, "restore_registry_state", err);
    }
    errors
}

fn restore_removed_links(removed_links: &[RemovedLink]) -> Vec<Value> {
    let mut errors = Vec::new();
    for removed in removed_links.iter().rev() {
        if rollback_fault_active("restore_projection_path") {
            push_rollback_error(
                &mut errors,
                "restore_projection_path",
                format!("fault injected for {}", removed.path.display()),
            );
            continue;
        }
        if let Some(parent) = removed.path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            push_rollback_error(&mut errors, "restore_projection_path", err);
            continue;
        }
        if let Err(err) = create_symlink_dir(&removed.target, &removed.path) {
            push_rollback_error(&mut errors, "restore_projection_path", err);
        }
    }
    errors
}

fn empty_plan() -> TrashActivationPlan {
    TrashActivationPlan {
        impact: TrashActivationImpact {
            removed_rule_count: 0,
            removed_projection_ids: Vec::new(),
            links: Vec::new(),
        },
        original_bindings: None,
        original_rules: None,
        original_projections: None,
        next_rules: None,
        next_projections: None,
        deletable_links: Vec::new(),
        skill_path: PathBuf::new(),
    }
}
