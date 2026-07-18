use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::ProjectionMethod;
use crate::envelope::Meta;
use crate::fs_util::{remove_path_if_exists, rename_atomic};
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
#[allow(
    dead_code,
    reason = "the convergence variant is consumed by the SP524-T004 transaction"
)]
pub(crate) enum ProjectionExecutionContext {
    Standalone,
    Convergence,
}

mod convergence;
#[cfg(test)]
mod tests;
#[allow(unused_imports)]
pub(crate) use convergence::{
    PreparedProjection, ProjectionActivationOutput, activate_prepared_projection,
    discard_prepared_projection,
};
use convergence::{map_ownership_fingerprint_error, projection_ownership_fingerprint};

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
    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) prepared: Option<PreparedProjection>,
    pub(crate) backup: Option<Value>,
    pub(crate) commit: Option<String>,
    pub(crate) meta: Meta,
    pub(crate) noop: bool,
}

pub(crate) struct StandaloneProjectionExecutionOutput {
    pub(crate) projection: Option<RegistryProjectionInstance>,
    pub(crate) backup: Option<Value>,
    pub(crate) commit: Option<String>,
    pub(crate) meta: Meta,
    pub(crate) noop: bool,
}

struct MaterializationResult {
    changed: bool,
    backup: Option<Value>,
    prepared: Option<PreparedProjection>,
    observation: Option<super::projections::ProjectionObservation>,
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

#[allow(
    dead_code,
    reason = "the generic convergence entry point is consumed by SP524-T004"
)]
pub(crate) fn execute_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> std::result::Result<ProjectionExecutionOutput, CommandFailure> {
    match input.context {
        ProjectionExecutionContext::Standalone => {
            execute_projection_mode::<false>(ctx, paths, snapshot, input)
        }
        ProjectionExecutionContext::Convergence => {
            execute_projection_mode::<true>(ctx, paths, snapshot, input)
        }
    }
}

fn execute_projection_mode<const CONVERGENCE: bool>(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> std::result::Result<ProjectionExecutionOutput, CommandFailure> {
    validate_execution_input(ctx, &input)?;

    // Resolve repository guards before convergence creates staging or mutates
    // a live path. No later read-only HEAD failure can strand a projection.
    let head = gitops::head(ctx).map_err(map_git)?;

    let existing_rule = find_rule(snapshot, &input.binding, &input.target, &input.skill).cloned();
    let existing_projection =
        find_projection(snapshot, &input.binding, &input.target, &input.skill).cloned();
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
    let materialization = materialize_projection::<CONVERGENCE>(
        ctx,
        &input,
        existing_projection.as_ref(),
        &projection,
    )?;

    let state_changed = input.target_is_new
        || input.binding_is_new
        || rule_needs_update(existing_rule.as_ref(), &input)
        || projection_needs_update(existing_projection.as_ref(), &input)
        || materialization.changed;

    if input.safe_existing_noop && !state_changed {
        return Ok(ProjectionExecutionOutput {
            projection: existing_projection,
            prepared: materialization.prepared,
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

    let observation = materialization
        .observation
        .unwrap_or_else(|| observe_projection(ctx, &projection));
    apply_projection_observation(&mut projection, &observation);

    if CONVERGENCE {
        if observation.status != "healthy" {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "convergence projection '{}' failed post-materialization validation: {}",
                    projection.instance_id,
                    observation
                        .error_code
                        .unwrap_or("projection_validation_failed")
                ),
            ));
        }
        return Ok(ProjectionExecutionOutput {
            projection: Some(projection),
            prepared: materialization.prepared,
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
        prepared: None,
        backup: materialization.backup,
        commit,
        meta,
        noop: false,
    })
}

pub(crate) fn execute_standalone_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> std::result::Result<StandaloneProjectionExecutionOutput, CommandFailure> {
    let output = execute_projection_mode::<false>(ctx, paths, snapshot, input)?;
    debug_assert!(output.prepared.is_none());
    Ok(StandaloneProjectionExecutionOutput {
        projection: output.projection,
        backup: output.backup,
        commit: output.commit,
        meta: output.meta,
        noop: output.noop,
    })
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

fn materialize_projection<const CONVERGENCE: bool>(
    ctx: &AppContext,
    input: &ProjectionExecutionInput,
    existing_projection: Option<&RegistryProjectionInstance>,
    projection: &RegistryProjectionInstance,
) -> std::result::Result<MaterializationResult, CommandFailure> {
    let target_base = PathBuf::from(&input.target.path);
    fs::create_dir_all(&target_base).map_err(map_io)?;
    let skill_src = ctx.skill_path(&input.skill);
    let path_exists =
        input.materialized_path.exists() || fs::symlink_metadata(&input.materialized_path).is_ok();
    let replace_existing = input.replace_existing || CONVERGENCE;

    if path_exists
        && matches!(input.method, ProjectionMethod::Symlink)
        && (CONVERGENCE || input.safe_existing_noop)
        && projection_path_is_safe_symlink(&input.materialized_path, &skill_src)
    {
        return Ok(MaterializationResult {
            changed: false,
            backup: None,
            prepared: None,
            observation: None,
        });
    }

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
        if !CONVERGENCE
            && input.safe_existing_noop
            && existing_projection.is_some_and(|projection| projection.method == input.method)
            && !matches!(input.method, ProjectionMethod::Symlink)
        {
            return Ok(MaterializationResult {
                changed: false,
                backup: None,
                prepared: None,
                observation: None,
            });
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

    let existing_digest = if CONVERGENCE && path_exists {
        Some(
            projection_ownership_fingerprint(&input.materialized_path)
                .map_err(|err| map_ownership_fingerprint_error(err, &input.materialized_path))?,
        )
    } else {
        None
    };

    let staging_path = target_base.join(format!(
        ".loom-projection-stage-{}",
        uuid::Uuid::new_v4().simple()
    ));
    if let Err(err) = project_skill_to_target(&skill_src, &staging_path, input.method) {
        let mut cleanup_errors = Vec::new();
        cleanup_projection_staging(&staging_path, &mut cleanup_errors);
        return Err(map_project_io(input.method)(err).with_rollback_errors(cleanup_errors));
    }

    if CONVERGENCE && let Err(err) = inject_convergence_staging_mismatch(input, &staging_path) {
        let mut cleanup_errors = Vec::new();
        cleanup_projection_staging(&staging_path, &mut cleanup_errors);
        return Err(err.with_rollback_errors(cleanup_errors));
    }

    if CONVERGENCE {
        let observation = observe_projection_path(ctx, projection, &staging_path);
        if observation.status != "healthy" {
            let mut cleanup_errors = Vec::new();
            cleanup_projection_staging(&staging_path, &mut cleanup_errors);
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "convergence projection '{}' failed staging validation: {}",
                    projection.instance_id,
                    observation
                        .error_code
                        .unwrap_or("projection_validation_failed")
                ),
            )
            .with_rollback_errors(cleanup_errors));
        }
        let mut prepared_projection = projection.clone();
        apply_projection_observation(&mut prepared_projection, &observation);
        let staging_digest = projection_ownership_fingerprint(&staging_path).map_err(|err| {
            let mut cleanup_errors = Vec::new();
            cleanup_projection_staging(&staging_path, &mut cleanup_errors);
            map_ownership_fingerprint_error(err, &staging_path).with_rollback_errors(cleanup_errors)
        })?;
        return Ok(MaterializationResult {
            changed: true,
            backup: None,
            prepared: Some(PreparedProjection::new(
                prepared_projection,
                staging_path,
                input.materialized_path.clone(),
                path_exists,
                staging_digest,
                existing_digest,
            )),
            observation: Some(observation),
        });
    }

    let persistent_backup = if path_exists && replace_existing && !CONVERGENCE {
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
        replace_standalone_projection(&staging_path, &input.materialized_path, persistent_backup)?
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
        prepared: None,
        observation: None,
    })
}

fn observe_projection_path(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
    path: &Path,
) -> super::projections::ProjectionObservation {
    let mut staged = projection.clone();
    staged.materialized_path = path.display().to_string();
    observe_projection(ctx, &staged)
}

#[cfg(test)]
fn inject_convergence_staging_mismatch(
    input: &ProjectionExecutionInput,
    staging_path: &Path,
) -> std::result::Result<(), CommandFailure> {
    if input.context == ProjectionExecutionContext::Convergence
        && input.after_materialize_fault == Some("test_convergence_staging_mismatch")
        && !matches!(input.method, ProjectionMethod::Symlink)
    {
        fs::write(staging_path.join("details.txt"), "fault-injected drift\n").map_err(map_io)?;
    }
    Ok(())
}

#[cfg(not(test))]
fn inject_convergence_staging_mismatch(
    _input: &ProjectionExecutionInput,
    _staging_path: &Path,
) -> std::result::Result<(), CommandFailure> {
    Ok(())
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
        if !rollback_fault_active("restore_projection_path")
            && let Err(err) = restore_path_from_backup(materialized_path, backup)
        {
            push_rollback_error(&mut errors, "restore_projection_path", err);
        }
    } else if !rollback_fault_active("remove_projection_path")
        && let Err(err) = remove_path_if_exists(materialized_path)
    {
        push_rollback_error(&mut errors, "remove_projection_path", err);
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
