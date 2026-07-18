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
    reason = "the convergence variant is consumed by SP524-T004"
)]
pub(crate) enum ProjectionExecutionContext {
    Standalone,
    Convergence,
}

mod convergence;
mod modes;
mod prepared;
mod staging_cleanup;
#[cfg(test)]
mod tests;
#[cfg(test)]
use convergence::activate_after_mutation;
#[allow(unused_imports)]
pub(crate) use convergence::{
    PreparedProjection, PreparedProjectionArtifact, ProjectionActivationOutput,
    ProjectionRollbackArtifact, activate_prepared_projection, discard_prepared_projection,
};
use convergence::{map_ownership_fingerprint_error, projection_ownership_fingerprint};
#[cfg(test)]
pub(crate) use prepared::execute_projection;
pub(crate) use prepared::{
    convergence_projection_fingerprint, execute_prepared_convergence_projection,
    prepare_convergence_projection,
};
use staging_cleanup::{
    StagingOwnership, cleanup_owned_staging, inject_convergence_anchor_failure,
    inject_convergence_project_failure_replacement, inject_convergence_staging_mismatch,
    observe_projection_path, preserve_unverified_staging,
};

pub(crate) struct ProjectionExecutionInput {
    pub(crate) context: ProjectionExecutionContext,
    pub(crate) skill: String,
    pub(crate) binding: RegistryWorkspaceBinding,
    pub(crate) binding_is_new: bool,
    pub(crate) target: RegistryProjectionTarget,
    pub(crate) target_is_new: bool,
    pub(crate) source_path: Option<PathBuf>,
    pub(crate) staging_path: Option<PathBuf>,
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

#[allow(dead_code, reason = "consumed by the SP524-T004 transaction")]
pub(crate) struct ProjectionExecutionOutput {
    pub(crate) projection: Option<RegistryProjectionInstance>,
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

struct MaterializationResult<P> {
    changed: bool,
    backup: Option<Value>,
    prepared: P,
    observation: Option<super::projections::ProjectionObservation>,
}

trait ExecutionMode {
    const CONVERGENCE: bool;
    type Prepared;
    type Output;

    fn none() -> Self::Prepared;
    fn prepared(prepared: PreparedProjection) -> Self::Prepared;
    fn output(
        projection: Option<RegistryProjectionInstance>,
        prepared: Self::Prepared,
        backup: Option<Value>,
        commit: Option<String>,
        meta: Meta,
        noop: bool,
    ) -> Self::Output;
}

struct StandaloneMode;
struct ConvergenceMode;

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

fn execute_projection_mode<M: ExecutionMode>(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
    prepared_staging: Option<PathBuf>,
) -> std::result::Result<M::Output, CommandFailure> {
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
    let now = Utc::now();
    let mut projection = RegistryProjectionInstance {
        instance_id: instance_id.clone(),
        skill_id: input.skill.clone(),
        binding_id: Some(input.binding.binding_id.clone()),
        target_id: input.target.target_id.clone(),
        materialized_path: input.materialized_path.display().to_string(),
        method: input.method,
        last_applied_rev: head,
        health: crate::core::vocab::Health::Healthy,
        observed_drift: Some(false),
        source_tree_digest: None,
        materialized_tree_digest: None,
        last_observed_at: None,
        last_observed_error: None,
        updated_at: Some(now),
    };
    let materialization = materialize_projection::<M>(
        ctx,
        &input,
        existing_projection.as_ref(),
        &projection,
        prepared_staging,
    )?;

    let state_changed = input.target_is_new
        || input.binding_is_new
        || rule_needs_update(existing_rule.as_ref(), &input)
        || projection_needs_update(existing_projection.as_ref(), &input)
        || materialization.changed;

    if input.safe_existing_noop && !state_changed {
        return Ok(M::output(
            existing_projection,
            materialization.prepared,
            materialization.backup,
            None,
            Meta::default(),
            true,
        ));
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
                .or(Some(now)),
        },
    );

    let observation = if M::CONVERGENCE {
        materialization
            .observation
            .unwrap_or_else(|| observe_projection(ctx, &projection))
    } else {
        observe_projection(ctx, &projection)
    };
    apply_projection_observation(&mut projection, &observation);

    if M::CONVERGENCE {
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
        return Ok(M::output(
            Some(projection),
            materialization.prepared,
            materialization.backup,
            None,
            Meta::default(),
            !materialization.changed && !state_changed,
        ));
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

    Ok(M::output(
        Some(projection),
        materialization.prepared,
        materialization.backup,
        commit,
        meta,
        false,
    ))
}

#[inline(always)]
pub(crate) fn execute_standalone_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> std::result::Result<StandaloneProjectionExecutionOutput, CommandFailure> {
    debug_assert_eq!(input.context, ProjectionExecutionContext::Standalone);
    execute_projection_mode::<StandaloneMode>(ctx, paths, snapshot, input, None)
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
                "target '{}' has ownership '{}' and cannot be written",
                input.target.target_id, input.target.ownership
            ),
        ));
    }
    validate_projection_method(&input.target, input.method)?;
    enforce_skill_safety(ctx, &input.skill, &input.binding.policy_profile)?;
    Ok(())
}

fn materialize_projection<M: ExecutionMode>(
    ctx: &AppContext,
    input: &ProjectionExecutionInput,
    existing_projection: Option<&RegistryProjectionInstance>,
    projection: &RegistryProjectionInstance,
    prepared_staging: Option<PathBuf>,
) -> std::result::Result<MaterializationResult<M::Prepared>, CommandFailure> {
    let target_base = PathBuf::from(&input.target.path);
    fs::create_dir_all(&target_base).map_err(map_io)?;
    let canonical_skill_src = ctx.skill_path(&input.skill);
    let skill_src = if M::CONVERGENCE {
        input.source_path.as_deref().unwrap_or(&canonical_skill_src)
    } else {
        &canonical_skill_src
    };
    let path_exists =
        input.materialized_path.exists() || fs::symlink_metadata(&input.materialized_path).is_ok();
    let replace_existing = input.replace_existing || M::CONVERGENCE;

    if path_exists
        && matches!(input.method, ProjectionMethod::Symlink)
        && (M::CONVERGENCE || input.safe_existing_noop)
        && projection_path_is_safe_symlink(&input.materialized_path, skill_src)
    {
        return Ok(MaterializationResult {
            changed: false,
            backup: None,
            prepared: M::none(),
            observation: None,
        });
    }

    if matches!(input.method, ProjectionMethod::Symlink) {
        let probe = probe_symlink(&target_base);
        if !probe.supported {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionMethodUnsupported,
                format!(
                    "target '{}' does not support symlink projections: {}",
                    input.target.target_id,
                    probe.reason.unwrap_or_else(|| "unknown reason".to_string())
                ),
            ));
        }
    }

    if path_exists {
        if !M::CONVERGENCE
            && input.safe_existing_noop
            && existing_projection.is_some_and(|projection| projection.method == input.method)
            && !matches!(input.method, ProjectionMethod::Symlink)
        {
            return Ok(MaterializationResult {
                changed: false,
                backup: None,
                prepared: M::none(),
                observation: None,
            });
        }
        if !replace_existing {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "path '{}' is not a safe Loom-owned {} projection",
                    input.materialized_path.display(),
                    projection_method_as_str(input.method)
                ),
            ));
        }
    }

    let existing_digest = if M::CONVERGENCE && path_exists {
        Some(
            projection_ownership_fingerprint(&input.materialized_path)
                .map_err(|err| map_ownership_fingerprint_error(err, &input.materialized_path))?,
        )
    } else {
        None
    };

    if prepared_staging.is_some() && !M::CONVERGENCE {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            "prepared staging requires convergence mode",
        ));
    }
    let has_prepared_staging = prepared_staging.is_some();
    let staging_path = prepared_staging.unwrap_or_else(|| {
        if M::CONVERGENCE {
            input.staging_path.clone().unwrap_or_else(|| {
                target_base.join(format!(
                    ".loom-projection-stage-{}",
                    uuid::Uuid::new_v4().simple()
                ))
            })
        } else {
            target_base.join(format!(
                ".loom-projection-stage-{}",
                uuid::Uuid::new_v4().simple()
            ))
        }
    });
    if M::CONVERGENCE
        && !has_prepared_staging
        && (staging_path == input.materialized_path
            || staging_path.parent() != input.materialized_path.parent())
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "staging path must be a distinct sibling",
        ));
    }
    let staging_exists = fs::symlink_metadata(&staging_path).is_ok();
    if M::CONVERGENCE && staging_exists && !has_prepared_staging {
        return Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "staging path '{}' exists; data was preserved",
                staging_path.display()
            ),
        ));
    }
    if has_prepared_staging && !staging_exists {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "prepared projection staging path is missing: {}",
                staging_path.display()
            ),
        ));
    }
    if !has_prepared_staging
        && let Err(err) = project_skill_to_target(skill_src, &staging_path, input.method)
    {
        if M::CONVERGENCE {
            inject_convergence_project_failure_replacement(input, &staging_path)?;
            return Err(preserve_unverified_staging(
                map_project_io(input.method)(err),
                &staging_path,
            ));
        }
        let mut cleanup_errors = Vec::new();
        cleanup_projection_staging(&staging_path, &mut cleanup_errors);
        return Err(map_project_io(input.method)(err).with_rollback_errors(cleanup_errors));
    }

    let staging_ownership = if M::CONVERGENCE {
        if let Err(err) = inject_convergence_anchor_failure(input, &staging_path) {
            return Err(preserve_unverified_staging(err, &staging_path));
        }
        match projection_ownership_fingerprint(&staging_path) {
            Ok(digest) => Some(StagingOwnership::new(staging_path.clone(), digest)),
            Err(err) => {
                return Err(preserve_unverified_staging(
                    map_ownership_fingerprint_error(err, &staging_path),
                    &staging_path,
                ));
            }
        }
    } else {
        None
    };

    if M::CONVERGENCE
        && let Err(err) = inject_convergence_staging_mismatch(input, &staging_path)
    {
        let cleanup_errors = cleanup_owned_staging(staging_ownership);
        return Err(err.with_rollback_errors(cleanup_errors));
    }

    if M::CONVERGENCE {
        let observation = observe_projection_path(projection, skill_src, &staging_path);
        if observation.status != "healthy" {
            let cleanup_errors = cleanup_owned_staging(staging_ownership);
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
        let staging_digest = staging_ownership
            .as_ref()
            .map(|ownership| ownership.digest().to_string())
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::InternalError,
                    "convergence staging ownership anchor is missing",
                )
            })?;
        return Ok(MaterializationResult {
            changed: true,
            backup: None,
            prepared: M::prepared(PreparedProjection::new(
                prepared_projection,
                skill_src.to_path_buf(),
                staging_path,
                input.materialized_path.clone(),
                path_exists,
                staging_digest,
                existing_digest,
            )),
            observation: Some(observation),
        });
    }

    let persistent_backup = if path_exists && replace_existing && !M::CONVERGENCE {
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
        prepared: M::none(),
        observation: None,
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
