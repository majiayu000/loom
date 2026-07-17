use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::ProjectionMethod;
use crate::envelope::Meta;
use crate::fs_util::{exchange_paths_atomic, remove_path_if_exists, rename_atomic};
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::{
    RegistryBindingRule, RegistryBindingsFile, RegistryProjectionInstance,
    RegistryProjectionTarget, RegistryProjectionsFile, RegistryRulesFile, RegistrySnapshot,
    RegistryStatePaths, RegistryTargetsFile, RegistryWorkspaceBinding,
};
use crate::types::ErrorCode;

use super::CommandFailure;
use super::codex_visibility::projection_path_is_safe_symlink;
use super::file_ops::{backup_path_if_exists, restore_path_from_backup};
use super::fs_probe::probe_symlink;
use super::helpers::{
    commit_registry_state, ensure_skill_exists, map_git, map_io, map_project_io,
    map_registry_state, projection_instance_id, projection_method_as_str,
    validate_projection_method,
};
use super::projections::{
    apply_projection_observation, maybe_autosync_or_queue, observe_projection,
    project_skill_to_target, record_registry_observation, record_registry_operation,
    restore_registry_audit_state, snapshot_registry_audit_state, upsert_projection, upsert_rule,
};
use super::skill_activation::resolve::{find_projection, find_rule};
use super::skill_cmds::shared::{
    maybe_skill_fault, push_rollback_error, rollback_fault_active, rollback_registry_state,
};
use super::skill_safety::enforce_skill_safety;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionExecutionContext {
    Standalone,
    Convergence,
}

#[cfg(test)]
mod tests;

pub(crate) struct ProjectionExecutionInput {
    pub(crate) context: ProjectionExecutionContext,
    pub(crate) skill: String,
    pub(crate) binding: RegistryWorkspaceBinding,
    pub(crate) binding_is_new: bool,
    pub(crate) target: RegistryProjectionTarget,
    pub(crate) target_is_new: bool,
    pub(crate) materialized_path: PathBuf,
    pub(crate) method: ProjectionMethod,
    pub(crate) operation_intent: &'static str,
    pub(crate) operation_payload: Value,
    pub(crate) observation_kind: &'static str,
    pub(crate) request_id: String,
    pub(crate) commit_message: String,
    pub(crate) replace_existing: bool,
    pub(crate) safe_existing_noop: bool,
    pub(crate) after_materialize_fault: Option<&'static str>,
    pub(crate) after_state_save_fault: Option<&'static str>,
    pub(crate) after_observation_fault: Option<&'static str>,
    pub(crate) activation_after_projection_fault: bool,
}

pub(crate) struct ProjectionExecutionOutput {
    pub(crate) projection: Option<RegistryProjectionInstance>,
    pub(crate) backup: Option<Value>,
    pub(crate) commit: Option<String>,
    pub(crate) meta: Meta,
    pub(crate) noop: bool,
}

struct MaterializationResult {
    changed: bool,
    backup: Option<Value>,
}

struct ProjectionRollback<'a> {
    paths: &'a RegistryStatePaths,
    materialized_path: &'a Path,
    backup: Option<&'a Value>,
    original_targets: &'a RegistryTargetsFile,
    original_bindings: &'a RegistryBindingsFile,
    original_rules: &'a RegistryRulesFile,
    original_projections: &'a RegistryProjectionsFile,
    targets_changed: bool,
}

pub(crate) fn execute_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> std::result::Result<ProjectionExecutionOutput, CommandFailure> {
    validate_execution_input(ctx, &input)?;

    let existing_rule = find_rule(snapshot, &input.binding, &input.target, &input.skill).cloned();
    let existing_projection =
        find_projection(snapshot, &input.binding, &input.target, &input.skill).cloned();
    let materialization = materialize_projection(ctx, &input, existing_projection.as_ref())?;

    let state_changed = input.target_is_new
        || input.binding_is_new
        || rule_needs_update(existing_rule.as_ref(), &input)
        || projection_needs_update(existing_projection.as_ref(), &input)
        || materialization.changed;

    if input.safe_existing_noop && !state_changed {
        return Ok(ProjectionExecutionOutput {
            projection: existing_projection,
            backup: materialization.backup,
            commit: None,
            meta: Meta::default(),
            noop: true,
        });
    }

    let original_targets = snapshot.targets.clone();
    let original_bindings = snapshot.bindings.clone();
    let original_rules = snapshot.rules.clone();
    let original_projections = snapshot.projections.clone();

    let mut targets = original_targets.clone();
    if input.target_is_new {
        targets.targets.push(input.target.clone());
        targets
            .targets
            .sort_by(|left, right| left.target_id.cmp(&right.target_id));
    }

    let mut bindings = original_bindings.clone();
    if input.binding_is_new {
        bindings.bindings.push(input.binding.clone());
        bindings
            .bindings
            .sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
    }

    let mut rules = original_rules.clone();
    upsert_rule(
        &mut rules,
        RegistryBindingRule {
            binding_id: input.binding.binding_id.clone(),
            skill_id: input.skill.clone(),
            target_id: input.target.target_id.clone(),
            method: input.method,
            watch_policy: "observe_only".to_string(),
            created_at: existing_rule
                .as_ref()
                .and_then(|rule| rule.created_at)
                .or_else(|| Some(Utc::now())),
        },
    );

    let head = gitops::head(ctx).map_err(map_git)?;
    let instance_id = projection_instance_id(
        &input.skill,
        &input.binding.binding_id,
        &input.target.target_id,
    );
    let mut projection = RegistryProjectionInstance {
        instance_id: instance_id.clone(),
        skill_id: input.skill.clone(),
        binding_id: Some(input.binding.binding_id.clone()),
        target_id: input.target.target_id.clone(),
        materialized_path: input.materialized_path.display().to_string(),
        method: input.method,
        last_applied_rev: head.clone(),
        health: crate::core::vocab::Health::Healthy,
        observed_drift: Some(false),
        source_tree_digest: None,
        materialized_tree_digest: None,
        last_observed_at: None,
        last_observed_error: None,
        updated_at: Some(Utc::now()),
    };
    if let Err(err) = inject_convergence_observation_mismatch(&input) {
        let rollback_errors = rollback_live_projection_path(
            &input.materialized_path,
            materialization.backup.as_ref(),
        );
        return Err(err.with_rollback_errors(rollback_errors));
    }
    let observation = observe_projection(ctx, &projection);
    apply_projection_observation(&mut projection, &observation);

    if input.context == ProjectionExecutionContext::Convergence {
        if observation.status != "healthy" {
            let rollback_errors = rollback_live_projection_path(
                &input.materialized_path,
                materialization.backup.as_ref(),
            );
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "convergence projection '{}' failed post-materialization validation: {}",
                    projection.instance_id,
                    observation
                        .error_code
                        .unwrap_or("projection_validation_failed")
                ),
            )
            .with_rollback_errors(rollback_errors));
        }
        return Ok(ProjectionExecutionOutput {
            projection: Some(projection),
            backup: materialization.backup,
            commit: None,
            meta: Meta::default(),
            noop: !materialization.changed && !state_changed,
        });
    }

    let mut projections = original_projections.clone();
    upsert_projection(&mut projections, projection.clone());

    let registry_audit_backup = snapshot_registry_audit_state(paths).map_err(map_registry_state)?;
    let post_materialize: std::result::Result<(Option<String>, Meta), CommandFailure> = (|| {
        if input.activation_after_projection_fault {
            maybe_activation_projection_fault(&input.skill)?;
        }
        if let Some(tag) = input.after_materialize_fault {
            maybe_skill_fault(tag)?;
        }
        save_projection_state(
            paths,
            &targets,
            &bindings,
            &rules,
            &projections,
            &original_targets,
            input.target_is_new,
        )?;
        if let Some(tag) = input.after_state_save_fault {
            maybe_skill_fault(tag)?;
        }
        let op_id = record_registry_operation(
            paths,
            input.operation_intent,
            input.operation_payload.clone(),
            json!({ "instance_id": instance_id }),
        )
        .map_err(map_registry_state)?;
        record_registry_observation(
            paths,
            &projection.instance_id,
            input.observation_kind,
            Some(projection.materialized_path.clone()),
            None,
            Some(projection.last_applied_rev.clone()),
        )
        .map_err(map_registry_state)?;
        if let Some(tag) = input.after_observation_fault {
            maybe_skill_fault(tag)?;
        }
        let commit = commit_registry_state(ctx, &input.commit_message)?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                ctx,
                input.operation_intent,
                &input.request_id,
                json!({
                    "skill": input.skill.clone(),
                    "binding_id": input.binding.binding_id.clone(),
                    "target_id": input.target.target_id.clone(),
                    "commit": commit
                }),
                &mut meta,
            )?;
        }
        Ok((commit, meta))
    })();

    let (commit, meta) = match post_materialize {
        Ok(result) => result,
        Err(err) => {
            let mut rollback_errors = Vec::new();
            if let Err(restore_err) = restore_registry_audit_state(paths, &registry_audit_backup) {
                push_rollback_error(
                    &mut rollback_errors,
                    "restore_registry_audit_state",
                    restore_err,
                );
            }
            rollback_errors.extend(rollback_projection_mutation(ProjectionRollback {
                paths,
                materialized_path: &input.materialized_path,
                backup: materialization.backup.as_ref(),
                original_targets: &original_targets,
                original_bindings: &original_bindings,
                original_rules: &original_rules,
                original_projections: &original_projections,
                targets_changed: input.target_is_new,
            }));
            return Err(err.with_rollback_errors(rollback_errors));
        }
    };

    Ok(ProjectionExecutionOutput {
        projection: Some(projection),
        backup: materialization.backup,
        commit,
        meta,
        noop: false,
    })
}

#[cfg(test)]
fn inject_convergence_observation_mismatch(
    input: &ProjectionExecutionInput,
) -> std::result::Result<(), CommandFailure> {
    if input.context == ProjectionExecutionContext::Convergence
        && input.after_materialize_fault == Some("test_convergence_observation_mismatch")
    {
        fs::write(
            input.materialized_path.join("details.txt"),
            "fault-injected drift\n",
        )
        .map_err(map_io)?;
    }
    Ok(())
}

#[cfg(not(test))]
fn inject_convergence_observation_mismatch(
    _input: &ProjectionExecutionInput,
) -> std::result::Result<(), CommandFailure> {
    Ok(())
}

fn validate_execution_input(
    ctx: &AppContext,
    input: &ProjectionExecutionInput,
) -> std::result::Result<(), CommandFailure> {
    ensure_skill_exists(ctx, &input.skill)?;
    if input.target.agent != input.binding.agent {
        return Err(CommandFailure::new(
            ErrorCode::TargetAgentMismatch,
            format!(
                "binding '{}' is for agent '{}' but target '{}' is for agent '{}'",
                input.binding.binding_id,
                input.binding.agent,
                input.target.target_id,
                input.target.agent
            ),
        ));
    }
    if input.target.ownership != crate::core::vocab::Ownership::Managed {
        return Err(CommandFailure::new(
            ErrorCode::TargetNotManaged,
            format!(
                "target '{}' has ownership '{}' and cannot be projected into",
                input.target.target_id, input.target.ownership
            ),
        ));
    }
    validate_projection_method(&input.target, input.method)?;
    enforce_skill_safety(ctx, &input.skill, &input.binding.policy_profile)?;
    Ok(())
}

fn materialize_projection(
    ctx: &AppContext,
    input: &ProjectionExecutionInput,
    existing_projection: Option<&RegistryProjectionInstance>,
) -> std::result::Result<MaterializationResult, CommandFailure> {
    let target_base = PathBuf::from(&input.target.path);
    fs::create_dir_all(&target_base).map_err(map_io)?;
    let skill_src = ctx.skill_path(&input.skill);
    let path_exists =
        input.materialized_path.exists() || fs::symlink_metadata(&input.materialized_path).is_ok();
    let safe_existing_noop = input.safe_existing_noop
        || (input.context == ProjectionExecutionContext::Convergence
            && matches!(input.method, ProjectionMethod::Symlink));
    let replace_existing =
        input.replace_existing || input.context == ProjectionExecutionContext::Convergence;

    if matches!(input.method, ProjectionMethod::Symlink) {
        let probe = probe_symlink(&target_base);
        if !probe.supported {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionMethodUnsupported,
                format!(
                    "target '{}' filesystem does not support symlink projection: {}. retry with --method copy",
                    input.target.target_id,
                    probe.reason.unwrap_or_else(|| "unknown reason".to_string())
                ),
            ));
        }
    }

    if path_exists {
        if safe_existing_noop {
            if matches!(input.method, ProjectionMethod::Symlink)
                && projection_path_is_safe_symlink(&input.materialized_path, &skill_src)
            {
                return Ok(MaterializationResult {
                    changed: false,
                    backup: None,
                });
            }
            if existing_projection.is_some_and(|projection| projection.method == input.method)
                && !matches!(input.method, ProjectionMethod::Symlink)
            {
                return Ok(MaterializationResult {
                    changed: false,
                    backup: None,
                });
            }
        }
        if !replace_existing {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "projection path '{}' already exists and is not a safe Loom-owned {} projection",
                    input.materialized_path.display(),
                    projection_method_as_str(input.method)
                ),
            ));
        }
    }

    let staging_path = target_base.join(format!(
        ".loom-projection-stage-{}",
        uuid::Uuid::new_v4().simple()
    ));
    if let Err(err) = project_skill_to_target(&skill_src, &staging_path, input.method) {
        let mut cleanup_errors = Vec::new();
        cleanup_projection_staging(&staging_path, &mut cleanup_errors);
        return Err(map_project_io(input.method)(err).with_rollback_errors(cleanup_errors));
    }

    let persistent_backup = if path_exists
        && replace_existing
        && input.context == ProjectionExecutionContext::Standalone
    {
        match backup_path_if_exists(ctx, &input.materialized_path, "project.replace_projection") {
            Ok(backup) => backup,
            Err(err) => {
                let mut cleanup_errors = Vec::new();
                cleanup_projection_staging(&staging_path, &mut cleanup_errors);
                return Err(map_io(err).with_rollback_errors(cleanup_errors));
            }
        }
    } else {
        None
    };
    let backup = if path_exists {
        if input.context == ProjectionExecutionContext::Convergence {
            if let Err(err) = exchange_paths_atomic(&staging_path, &input.materialized_path) {
                let mut cleanup_errors = Vec::new();
                cleanup_projection_staging(&staging_path, &mut cleanup_errors);
                return Err(map_io(err).with_rollback_errors(cleanup_errors));
            }
            Some(atomic_exchange_backup(
                &input.materialized_path,
                &staging_path,
            ))
        } else {
            replace_standalone_projection(
                &staging_path,
                &input.materialized_path,
                persistent_backup,
            )?
        }
    } else {
        if let Err(err) = rename_atomic(&staging_path, &input.materialized_path) {
            let mut cleanup_errors = Vec::new();
            cleanup_projection_staging(&staging_path, &mut cleanup_errors);
            return Err(map_io(err).with_rollback_errors(cleanup_errors));
        }
        None
    };

    Ok(MaterializationResult {
        changed: true,
        backup,
    })
}

fn replace_standalone_projection(
    staging_path: &Path,
    materialized_path: &Path,
    backup: Option<Value>,
) -> Result<Option<Value>, CommandFailure> {
    if let Err(err) = remove_path_if_exists(materialized_path) {
        let mut rollback_errors = rollback_live_projection_path(materialized_path, backup.as_ref());
        cleanup_projection_staging(staging_path, &mut rollback_errors);
        return Err(map_io(err).with_rollback_errors(rollback_errors));
    }
    if let Err(err) = rename_atomic(staging_path, materialized_path) {
        let mut rollback_errors = rollback_live_projection_path(materialized_path, backup.as_ref());
        cleanup_projection_staging(staging_path, &mut rollback_errors);
        return Err(map_io(err).with_rollback_errors(rollback_errors));
    }
    Ok(backup)
}

fn cleanup_projection_staging(path: &Path, errors: &mut Vec<Value>) {
    if let Err(err) = remove_path_if_exists(path) {
        push_rollback_error(errors, "remove_projection_staging", err);
    }
}

fn atomic_exchange_backup(materialized_path: &Path, backup_path: &Path) -> Value {
    json!({
        "reason": "convergence.atomic_exchange",
        "kind": "atomic_exchange",
        "original_path": materialized_path.display().to_string(),
        "backup_path": backup_path.display().to_string(),
    })
}

fn rollback_atomic_exchange(materialized_path: &Path, backup_path: &Path) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Err(err) = exchange_paths_atomic(backup_path, materialized_path) {
        push_rollback_error(&mut errors, "restore_projection_atomic_exchange", err);
        return errors;
    }
    cleanup_projection_staging(backup_path, &mut errors);
    errors
}

fn save_projection_state(
    paths: &RegistryStatePaths,
    targets: &RegistryTargetsFile,
    bindings: &RegistryBindingsFile,
    rules: &RegistryRulesFile,
    projections: &RegistryProjectionsFile,
    original_targets: &RegistryTargetsFile,
    targets_changed: bool,
) -> std::result::Result<(), CommandFailure> {
    if targets_changed {
        paths.save_targets(targets).map_err(map_registry_state)?;
    }
    if let Err(err) = paths.save_bindings_rules_projections(bindings, rules, projections) {
        if targets_changed && let Err(restore_err) = paths.save_targets(original_targets) {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!(
                    "failed to save projection state and failed to rollback targets: {}; rollback error: {}",
                    err, restore_err
                ),
            ));
        }
        return Err(map_registry_state(err));
    }
    Ok(())
}

fn rollback_projection_mutation(rollback: ProjectionRollback<'_>) -> Vec<Value> {
    let mut errors = rollback_live_projection_path(rollback.materialized_path, rollback.backup);
    if rollback.targets_changed
        && let Err(err) = rollback.paths.save_targets(rollback.original_targets)
    {
        push_rollback_error(&mut errors, "restore_targets_state", err);
    }
    if let Err(err) = rollback_registry_state(
        rollback.paths,
        rollback.original_bindings,
        rollback.original_rules,
        rollback.original_projections,
    ) {
        push_rollback_error(&mut errors, "restore_registry_state", err);
    }
    errors
}

fn rollback_live_projection_path(materialized_path: &Path, backup: Option<&Value>) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Some(backup) = backup {
        if !rollback_fault_active("restore_projection_path") {
            if backup.get("kind").and_then(Value::as_str) == Some("atomic_exchange") {
                let Some(backup_path) = backup
                    .get("backup_path")
                    .and_then(Value::as_str)
                    .map(Path::new)
                else {
                    push_rollback_error(
                        &mut errors,
                        "restore_projection_atomic_exchange",
                        "atomic exchange backup is missing backup_path",
                    );
                    return errors;
                };
                errors.extend(rollback_atomic_exchange(materialized_path, backup_path));
            } else if let Err(err) = restore_path_from_backup(materialized_path, backup) {
                push_rollback_error(&mut errors, "restore_projection_path", err);
            }
        }
    } else if !rollback_fault_active("remove_projection_path")
        && let Err(err) = remove_path_if_exists(materialized_path)
    {
        push_rollback_error(&mut errors, "remove_projection_path", err);
    }
    errors
}

pub(crate) fn rollback_convergence_projection(
    materialized_path: &Path,
    backup: Option<&Value>,
) -> Vec<Value> {
    rollback_live_projection_path(materialized_path, backup)
}

pub(crate) fn finish_convergence_projection(backup: Option<&Value>) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Some(path) = backup
        .and_then(|value| value.get("backup_path"))
        .and_then(Value::as_str)
    {
        cleanup_projection_staging(Path::new(path), &mut errors);
    }
    errors
}

fn maybe_activation_projection_fault(skill: &str) -> std::result::Result<(), CommandFailure> {
    if let Ok(raw) = std::env::var("LOOM_SKILL_ACTIVATE_FAULT_INJECT")
        && raw == format!("after_projection:{skill}")
    {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            format!("fault injected after projecting {}", skill),
        ));
    }
    Ok(())
}

fn rule_needs_update(rule: Option<&RegistryBindingRule>, input: &ProjectionExecutionInput) -> bool {
    rule.is_none_or(|rule| rule.method != input.method)
}

fn projection_needs_update(
    projection: Option<&RegistryProjectionInstance>,
    input: &ProjectionExecutionInput,
) -> bool {
    projection.is_none_or(|projection| {
        projection.method != input.method
            || projection.materialized_path != input.materialized_path.display().to_string()
            || projection.health != crate::core::vocab::Health::Healthy
    })
}
