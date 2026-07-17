use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::ProjectionMethod;
use crate::core::convergence::{
    ConvergenceAxis, ConvergenceInputDirection, RemotePolicy, SkillConvergencePlan,
};
use crate::fs_util::{exchange_paths_atomic, remove_path_if_exists, write_atomic};
use crate::gitops;
use crate::state_model::{RegistryProjectionsFile, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::file_ops::{backup_path_if_exists, restore_path_from_backup};
use super::super::helpers::{commit_registry_state, map_git, map_io, map_lock, map_registry_state};
use super::super::projection_executor::{
    ProjectionExecutionContext, ProjectionExecutionInput, execute_projection,
    finish_convergence_projection, rollback_convergence_projection,
};
use super::super::projections::{project_skill_to_target, upsert_projection};
use super::super::provenance::skill_tree_digest;
use super::super::skill_cmds::shared::{maybe_skill_fault, push_rollback_error};
use super::super::{App, CommandFailure};
use super::{PLAN_PROTOCOL_VERSION, plan_failure};

const SCHEMA_VERSION: &str = "1.2";

#[derive(Debug, Serialize, Deserialize)]
struct TransactionJournal {
    plan_id: String,
    skill: String,
    previous_head: String,
    index_backup: String,
    source_backup: Option<Value>,
    source_staging: Option<String>,
    projections: Vec<ProjectionBackup>,
    original_projections: RegistryProjectionsFile,
    phase: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectionBackup {
    materialized_path: String,
    backup: Option<Value>,
}

pub(super) fn apply_convergence(
    app: &App,
    stored: &Value,
    cursor: usize,
    idempotency_key_digest: &str,
    request_id: &str,
) -> std::result::Result<Value, CommandFailure> {
    let plan: SkillConvergencePlan = serde_json::from_value(stored.clone()).map_err(|err| {
        plan_failure(
            ErrorCode::StateCorrupt,
            format!("stored convergence plan is invalid: {err}"),
            "PLAN_CORRUPT",
            false,
            vec!["create and review a fresh convergence plan".to_string()],
            Some(cursor),
        )
    })?;
    let _workspace_lock = app.ctx.lock_workspace().map_err(map_lock)?;
    let _skill_lock = app.ctx.lock_skill(&plan.skill).map_err(map_lock)?;
    let journal_path = journal_path(app, &plan.skill);
    if journal_path.exists() {
        recover_journal(app, &journal_path)?;
    }
    validate_guards(app, &plan, cursor)?;

    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
    let previous_index = gitops::snapshot_index(&app.ctx).map_err(map_git)?;
    let tx_dir = journal_path.parent().expect("journal has parent");
    fs::create_dir_all(tx_dir).map_err(map_io)?;
    let durable_index = tx_dir.join(format!("{}-index", plan.plan_id));
    fs::copy(previous_index.backup_path(), &durable_index).map_err(map_io)?;
    let source_backup = if plan.source.direction == ConvergenceInputDirection::Projection {
        backup_path_if_exists(
            &app.ctx,
            &app.ctx.skill_path(&plan.skill),
            "convergence.source",
        )
        .map_err(map_io)?
    } else {
        None
    };
    let source_staging =
        (plan.source.direction == ConvergenceInputDirection::Projection).then(|| {
            app.ctx
                .skills_dir
                .join(format!(
                    ".loom-convergence-source-stage-{}",
                    uuid::Uuid::new_v4().simple()
                ))
                .display()
                .to_string()
        });
    let projection_backups = plan
        .projections
        .iter()
        .map(|effect| {
            Ok(ProjectionBackup {
                materialized_path: effect.materialized_path.clone(),
                backup: backup_path_if_exists(
                    &app.ctx,
                    Path::new(&effect.materialized_path),
                    "convergence.projection",
                )
                .map_err(map_io)?,
            })
        })
        .collect::<std::result::Result<Vec<_>, CommandFailure>>()?;
    let mut journal = TransactionJournal {
        plan_id: plan.plan_id.clone(),
        skill: plan.skill.clone(),
        previous_head: plan.source.registry_head.clone(),
        index_backup: durable_index.display().to_string(),
        source_backup,
        source_staging,
        projections: projection_backups,
        original_projections: snapshot.projections.clone(),
        phase: "snapshotted".to_string(),
    };
    save_journal(&journal_path, &journal)?;

    let result = execute_local_transaction(
        app,
        &paths,
        &snapshot,
        &plan,
        request_id,
        &journal_path,
        &mut journal,
    );
    let output = match result {
        Ok(output) => output,
        Err(err) if interruption_fault_active() => return Err(err),
        Err(err) => {
            let rollback_errors = rollback_journal(app, &paths, &journal);
            if rollback_errors.is_empty() {
                cleanup_journal(&journal_path, &journal).map_err(map_io)?;
            }
            return Err(err.with_rollback_errors(rollback_errors));
        }
    };
    let cleanup_errors = finish_transaction(&journal);
    if !cleanup_errors.is_empty() {
        let mut rollback_errors = cleanup_errors;
        rollback_errors.extend(rollback_journal(app, &paths, &journal));
        return Err(CommandFailure::new(
            ErrorCode::IoError,
            "convergence transaction backup cleanup failed",
        )
        .with_rollback_errors(rollback_errors));
    }
    cleanup_journal(&journal_path, &journal).map_err(map_io)?;
    Ok(json!({
        "protocol_version": PLAN_PROTOCOL_VERSION,
        "schema_version": SCHEMA_VERSION,
        "plan_id": plan.plan_id,
        "idempotency_key_digest": idempotency_key_digest,
        "idempotent_replay": false,
        "plan_event_cursor": cursor,
        "applied": output,
        "recovery": { "rollback_supported": true },
    }))
}

fn execute_local_transaction(
    app: &App,
    paths: &RegistryStatePaths,
    snapshot: &crate::state_model::RegistrySnapshot,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Value, CommandFailure> {
    if plan.source.direction == ConvergenceInputDirection::Projection {
        replace_source_from_projection(app, plan, journal)?;
        journal.phase = "source_replaced".to_string();
        save_journal(journal_path, journal)?;
    }
    let source_commit = gitops::commit_paths_if_changed(
        &app.ctx,
        &[&format!("skills/{}", plan.skill)],
        &format!("skill({}): converge source", plan.skill),
    )
    .map_err(map_git)?;
    journal.phase = "source_committed".to_string();
    save_journal(journal_path, journal)?;
    maybe_skill_fault("convergence_interrupt_after_source_commit")?;
    maybe_skill_fault("convergence_after_source_commit")?;

    let mut projections = snapshot.projections.clone();
    let mut applied = Vec::new();
    for effect in &plan.projections {
        let binding = snapshot
            .binding(&effect.binding_id)
            .cloned()
            .ok_or_else(|| stale("planned binding no longer exists", "PLAN_BINDING_DRIFT"))?;
        let target = snapshot
            .target(&effect.target_id)
            .cloned()
            .ok_or_else(|| stale("planned target no longer exists", "PLAN_TARGET_DRIFT"))?;
        let method = parse_method(&effect.method)?;
        let output = execute_projection(
            &app.ctx,
            paths,
            snapshot,
            ProjectionExecutionInput {
                context: ProjectionExecutionContext::Convergence,
                skill: plan.skill.clone(),
                binding,
                binding_is_new: false,
                target,
                target_is_new: false,
                materialized_path: PathBuf::from(&effect.materialized_path),
                method,
                operation_intent: "converge",
                operation_payload: json!({}),
                observation_kind: "converge",
                request_id: request_id.to_string(),
                commit_message: String::new(),
                replace_existing: true,
                safe_existing_noop: false,
                after_materialize_fault: None,
                after_state_save_fault: None,
                after_observation_fault: None,
                activation_after_projection_fault: false,
            },
        )?;
        let projection = output.projection.ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "executor omitted projection state")
        })?;
        let cleanup_errors = finish_convergence_projection(output.backup.as_ref());
        if !cleanup_errors.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::IoError,
                "failed to retire atomic projection exchange backup",
            )
            .with_rollback_errors(cleanup_errors));
        }
        upsert_projection(&mut projections, projection.clone());
        applied.push(projection.instance_id);
        maybe_skill_fault("convergence_after_projection_swap")?;
    }
    journal.phase = "projections_swapped".to_string();
    save_journal(journal_path, journal)?;
    paths
        .save_projections(&projections)
        .map_err(map_registry_state)?;
    maybe_skill_fault("convergence_after_registry_save")?;
    let registry_commit = commit_registry_state(
        &app.ctx,
        &format!("skill({}): record convergence projections", plan.skill),
    )?;
    journal.phase = "registry_committed".to_string();
    save_journal(journal_path, journal)?;
    Ok(json!({
        "skill": plan.skill,
        "source_commit": source_commit,
        "registry_commit": registry_commit,
        "projection_instances": applied,
    }))
}

fn validate_guards(
    app: &App,
    plan: &SkillConvergencePlan,
    cursor: usize,
) -> std::result::Result<(), CommandFailure> {
    if !plan.input_conflicts.is_empty() || !plan.preflight.mutation_allowed {
        return Err(plan_failure(
            ErrorCode::PolicyBlocked,
            "convergence plan contains unresolved conflicts",
            "PLAN_NOT_SAFE_TO_APPLY",
            false,
            vec!["resolve conflicts and create a fresh plan".to_string()],
            Some(cursor),
        ));
    }
    if plan.remote != RemotePolicy::NotRequested
        || plan.required_axes.contains(&ConvergenceAxis::Visibility)
    {
        return Err(plan_failure(
            ErrorCode::PolicyBlocked,
            "requested post-local convergence axes are not executable in this tranche",
            "CONVERGENCE_POST_LOCAL_UNAVAILABLE",
            false,
            vec!["create a local-only convergence plan".to_string()],
            Some(cursor),
        ));
    }
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head != plan.source.registry_head {
        return Err(stale("registry HEAD changed after planning", "PLAN_STALE"));
    }
    let source_digest = skill_tree_digest(&app.ctx.skill_path(&plan.skill)).map_err(map_io)?;
    if source_digest != plan.source.tree_digest {
        return Err(stale(
            "canonical source changed after planning",
            "PLAN_SOURCE_DRIFT",
        ));
    }
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    if plan.registry.initialized != snapshot.is_some() {
        return Err(stale(
            "registry initialization changed after planning",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    if let Some(snapshot) = snapshot {
        let digest = digest_value(&snapshot.checkpoint)?;
        if plan.registry.checkpoint_digest.as_deref() != Some(digest.as_str())
            || plan.registry.checkpoint_updated_at.as_deref()
                != Some(snapshot.checkpoint.updated_at.to_rfc3339().as_str())
        {
            return Err(stale(
                "registry checkpoint changed after planning",
                "PLAN_CHECKPOINT_DRIFT",
            ));
        }
        for effect in &plan.projections {
            let binding = snapshot
                .binding(&effect.binding_id)
                .ok_or_else(|| stale("planned binding no longer exists", "PLAN_BINDING_DRIFT"))?;
            let target = snapshot
                .target(&effect.target_id)
                .ok_or_else(|| stale("planned target no longer exists", "PLAN_TARGET_DRIFT"))?;
            if binding.agent.as_str() != effect.agent
                || binding.profile_id != effect.profile
                || target.agent.as_str() != effect.agent
                || target.ownership.as_str() != effect.ownership
                || Path::new(&target.path).join(&plan.skill) != Path::new(&effect.materialized_path)
            {
                return Err(stale(
                    "projection routing changed after planning",
                    "PLAN_PROJECTION_DRIFT",
                ));
            }
            let live = skill_tree_digest(Path::new(&effect.materialized_path)).map_err(map_io)?;
            if effect.materialized_tree_digest.as_deref() != Some(live.as_str()) {
                return Err(stale(
                    "projection bytes changed after planning",
                    "PLAN_PROJECTION_DRIFT",
                ));
            }
        }
    }
    Ok(())
}

fn replace_source_from_projection(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let instance = plan.source.input_instance.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "projection input has no instance id",
        )
    })?;
    let effect = plan
        .projections
        .iter()
        .find(|effect| effect.instance_id == instance)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "projection input is absent from effects",
            )
        })?;
    let source = app.ctx.skill_path(&plan.skill);
    let staging = journal
        .source_staging
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "source staging path is absent")
        })?;
    project_skill_to_target(
        Path::new(&effect.materialized_path),
        &staging,
        ProjectionMethod::Copy,
    )
    .map_err(map_io)?;
    exchange_paths_atomic(&staging, &source).map_err(map_io)?;
    Ok(())
}

fn recover_journal(app: &App, journal_path: &Path) -> std::result::Result<(), CommandFailure> {
    let raw = fs::read_to_string(journal_path).map_err(map_io)?;
    let journal: TransactionJournal = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("invalid convergence journal: {err}"),
        )
    })?;
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let errors = rollback_journal(app, &paths, &journal);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "interrupted convergence recovery failed",
        )
        .with_rollback_errors(errors));
    }
    cleanup_journal(journal_path, &journal).map_err(map_io)
}

fn rollback_journal(
    app: &App,
    paths: &RegistryStatePaths,
    journal: &TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Err(err) = paths.save_projections(&journal.original_projections) {
        push_rollback_error(&mut errors, "restore_registry_projections", err);
    }
    for projection in journal.projections.iter().rev() {
        errors.extend(rollback_convergence_projection(
            Path::new(&projection.materialized_path),
            projection.backup.as_ref(),
        ));
    }
    if let Some(backup) = journal.source_backup.as_ref()
        && let Err(err) = restore_path_from_backup(&app.ctx.skill_path(&journal.skill), backup)
    {
        push_rollback_error(&mut errors, "restore_source_path", err);
    }
    if let Some(staging) = journal.source_staging.as_deref()
        && let Err(err) = remove_path_if_exists(Path::new(staging))
    {
        push_rollback_error(&mut errors, "remove_source_staging", err);
    }
    match gitops::run_git_allow_failure(&app.ctx, &["reset", "--soft", &journal.previous_head]) {
        Ok(output) if output.status.success() => {}
        Ok(output) => push_rollback_error(
            &mut errors,
            "restore_head",
            String::from_utf8_lossy(&output.stderr).trim(),
        ),
        Err(err) => push_rollback_error(&mut errors, "restore_head", err),
    }
    if let Err(err) = gitops::restore_index_from_backup(&app.ctx, Path::new(&journal.index_backup))
    {
        push_rollback_error(&mut errors, "restore_git_index", err);
    }
    errors
}

fn finish_transaction(journal: &TransactionJournal) -> Vec<Value> {
    let mut errors = Vec::new();
    for projection in &journal.projections {
        cleanup_persistent_backup(projection.backup.as_ref(), &mut errors);
    }
    cleanup_persistent_backup(journal.source_backup.as_ref(), &mut errors);
    if let Some(path) = journal.source_staging.as_deref()
        && let Err(err) = remove_path_if_exists(Path::new(path))
    {
        push_rollback_error(&mut errors, "remove_source_staging", err);
    }
    errors
}

fn cleanup_persistent_backup(backup: Option<&Value>, errors: &mut Vec<Value>) {
    if let Some(path) = backup
        .and_then(|value| value.get("backup_path"))
        .and_then(Value::as_str)
        && let Err(err) = remove_path_if_exists(Path::new(path))
    {
        push_rollback_error(errors, "remove_transaction_backup", err);
    }
}

fn save_journal(
    path: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string_pretty(journal).map_err(map_io)?;
    write_atomic(path, &(raw + "\n")).map_err(map_io)
}

fn cleanup_journal(path: &Path, journal: &TransactionJournal) -> std::io::Result<()> {
    let errors = finish_transaction(journal);
    if !errors.is_empty() {
        return Err(std::io::Error::other(
            serde_json::to_string(&errors).unwrap_or_else(|_| "backup cleanup failed".to_string()),
        ));
    }
    remove_path_if_exists(Path::new(&journal.index_backup))?;
    remove_path_if_exists(path)
}

fn journal_path(app: &App, skill: &str) -> PathBuf {
    app.ctx
        .state_dir
        .join("transactions")
        .join(format!("convergence-{skill}.json"))
}

fn parse_method(value: &str) -> std::result::Result<ProjectionMethod, CommandFailure> {
    match value {
        "symlink" => Ok(ProjectionMethod::Symlink),
        "copy" => Ok(ProjectionMethod::Copy),
        "materialize" => Ok(ProjectionMethod::Materialize),
        _ => Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("stored projection method '{value}' is invalid"),
        )),
    }
}

fn digest_value(value: &impl Serialize) -> std::result::Result<String, CommandFailure> {
    use crate::sha256::{Sha256, to_hex};
    let bytes = serde_json::to_vec(value).map_err(map_io)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn stale(message: impl Into<String>, code: &str) -> CommandFailure {
    plan_failure(
        ErrorCode::DependencyConflict,
        message,
        code,
        false,
        vec!["create and review a fresh convergence plan".to_string()],
        None,
    )
}

fn interruption_fault_active() -> bool {
    std::env::var("LOOM_FAULT_INJECT").ok().as_deref()
        == Some("convergence_interrupt_after_source_commit")
}
