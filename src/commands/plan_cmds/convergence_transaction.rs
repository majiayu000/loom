use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::ProjectionMethod;
use crate::core::convergence::{
    ConvergenceAxis, ConvergenceInputDirection, RemotePolicy, SkillConvergencePlan,
};
use crate::fs_util::{
    exchange_paths_atomic, remove_path_if_exists, rename_no_replace_atomic, write_atomic,
};
use crate::gitops;
use crate::state_model::{RegistryProjectionsFile, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::codex_visibility::projection_path_is_safe_symlink;
use super::super::file_ops::{create_declared_path_backup, restore_path_from_backup_if_absent};
use super::super::helpers::{map_git, map_io, map_lock, map_registry_state};
use super::super::projection_executor::{
    ProjectionExecutionContext, ProjectionExecutionInput, convergence_projection_fingerprint,
    execute_prepared_convergence_projection, finish_convergence_projection,
    prepare_convergence_projection,
};
use super::super::projections::{project_skill_to_target, upsert_projection};
use super::super::provenance::skill_tree_digest;
use super::super::skill_cmds::shared::{maybe_skill_fault, push_rollback_error};
use super::super::{App, CommandFailure};
use super::{PLAN_PROTOCOL_VERSION, plan_failure};

mod ownership;
mod projection_recovery;
mod recovery_evidence;
mod recovery_support;
mod rollback;
mod source_commit;
mod source_recovery;
use ownership::{
    cleanup_owned_dir, cleanup_reservation, owner_proof_is_valid, reservation_paths,
    validate_owned_staging, validate_transaction_artifacts,
};
use projection_recovery::{
    restore_projection_from_evidence, validate_projection_staging_fingerprint,
};
use recovery_evidence::{active_index_digest, file_digest, validate_rollback_evidence};
use recovery_support::*;
use rollback::{finish_transaction, rollback_journal};
use source_recovery::restore_source_from_evidence;

const SCHEMA_VERSION: &str = "1.2";

#[derive(Debug, Serialize, Deserialize)]
struct TransactionJournal {
    plan_id: String,
    skill: String,
    previous_head: String,
    artifact_root: String,
    artifact_owner_proof: String,
    index_backup: String,
    index_backup_digest: Option<String>,
    source_backup: Option<Value>,
    source_staging: Option<String>,
    source_owner_proof: Option<String>,
    #[serde(default)]
    source_activated_fingerprint: Option<String>,
    projections: Vec<ProjectionBackup>,
    original_projections: RegistryProjectionsFile,
    installed_projections: usize,
    expected_projections: Option<RegistryProjectionsFile>,
    source_head: Option<String>,
    source_commit: Option<String>,
    source_staged_index_digest: Option<String>,
    rollback_head: Option<String>,
    rollback_index_digest: Option<String>,
    result: Option<Value>,
    phase: TransactionPhase,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Preparing,
    Prepared,
    ReplacingSource,
    SourceReplaced,
    CommittingSource,
    SourceCommitted,
    InstallingProjections,
    ProjectionsSwapped,
    CommittingRegistry,
    RollingBack,
    CommittedCleanupPending,
    RolledBackCleanupPending,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProjectionBackup {
    materialized_path: String,
    backup: Option<Value>,
    staging_owner: String,
    owner_proof: String,
    staging_path: String,
    #[serde(default)]
    activated_fingerprint: Option<String>,
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
    if stored["safe_to_apply"] != json!(true) || !plan.required_approvals.is_empty() {
        return Err(plan_failure(
            ErrorCode::PolicyBlocked,
            "convergence policy approvals are not executable in this tranche",
            "CONVERGENCE_POLICY_WORKFLOW_REQUIRED",
            false,
            vec!["wait for the reviewed policy execution workflow".to_string()],
            Some(cursor),
        ));
    }
    let _workspace_lock = app.ctx.lock_workspace().map_err(map_lock)?;
    let _skill_lock = app.ctx.lock_skill(&plan.skill).map_err(map_lock)?;
    let journal_path = journal_path(app, &plan.skill);
    if journal_path.exists()
        && let Some(output) = recover_journal(app, &journal_path, &plan, request_id)?
    {
        return Ok(apply_output(&plan, cursor, idempotency_key_digest, output));
    }
    validate_guards(app, &plan, cursor)?;

    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
    let tx_dir = journal_path.parent().expect("journal has parent");
    fs::create_dir_all(tx_dir).map_err(map_io)?;
    let artifact_dir = tx_dir.join(format!("{}-artifacts", plan.plan_id));
    let durable_index = artifact_dir.join("index");
    let source_backup = (plan.source.direction == ConvergenceInputDirection::Projection)
        .then(|| {
            declared_backup(
                &app.ctx.skill_path(&plan.skill),
                &artifact_dir.join("source"),
            )
        })
        .transpose()?
        .flatten();
    let source_staging =
        (plan.source.direction == ConvergenceInputDirection::Projection).then(|| {
            app.ctx
                .skills_dir
                .join(format!(
                    ".loom-convergence-source-stage-{}.owner/stage",
                    plan.plan_id
                ))
                .display()
                .to_string()
        });
    let source_owner_proof = source_staging
        .as_ref()
        .map(|_| new_owner_proof(&plan.plan_id));
    let projection_backups = plan
        .projections
        .iter()
        .enumerate()
        .map(|(index, effect)| {
            let materialized = Path::new(&effect.materialized_path);
            let parent = materialized.parent().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "projection path has no parent")
            })?;
            let staging_owner = parent.join(format!(
                ".loom-projection-stage-{}-{index}.owner",
                plan.plan_id
            ));
            Ok(ProjectionBackup {
                materialized_path: effect.materialized_path.clone(),
                backup: declared_backup(
                    materialized,
                    &artifact_dir.join(format!("projection-{index}")),
                )?,
                staging_path: staging_owner.join("stage").display().to_string(),
                staging_owner: staging_owner.display().to_string(),
                owner_proof: new_owner_proof(&plan.plan_id),
                activated_fingerprint: None,
            })
        })
        .collect::<std::result::Result<Vec<_>, CommandFailure>>()?;
    let mut journal = TransactionJournal {
        plan_id: plan.plan_id.clone(),
        skill: plan.skill.clone(),
        previous_head: plan.source.registry_head.clone(),
        artifact_root: artifact_dir.display().to_string(),
        artifact_owner_proof: new_owner_proof(&plan.plan_id),
        index_backup: durable_index.display().to_string(),
        index_backup_digest: None,
        source_backup,
        source_staging,
        source_owner_proof,
        source_activated_fingerprint: None,
        projections: projection_backups,
        original_projections: snapshot.projections.clone(),
        installed_projections: 0,
        expected_projections: None,
        source_head: None,
        source_commit: None,
        source_staged_index_digest: None,
        rollback_head: None,
        rollback_index_digest: None,
        result: None,
        phase: TransactionPhase::Preparing,
    };
    save_journal(&journal_path, &journal)?;

    if let Err(err) = prepare_transaction_artifacts(app, &plan, &journal_path, &mut journal) {
        if interruption_fault_active() {
            return Err(err);
        }
        let cleanup_errors = cleanup_declared_artifacts(&journal_path, &journal);
        return Err(err.with_rollback_errors(cleanup_errors));
    }
    journal.phase = TransactionPhase::Prepared;
    save_journal(&journal_path, &journal)?;
    maybe_skill_fault("convergence_interrupt_after_prepared")?;

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
        Err(err) if interruption_fault_active() => {
            return Err(err);
        }
        Err(err) => {
            let rollback_head = gitops::head(&app.ctx).map_err(map_git)?;
            if rollback_head != journal.previous_head
                && journal.source_head.as_deref() != Some(rollback_head.as_str())
            {
                return Err(err.with_rollback_errors(vec![json!({
                    "step": "capture_rollback_head",
                    "message": "HEAD is neither old nor the recorded transaction head",
                })]));
            }
            journal.rollback_head = Some(rollback_head);
            journal.rollback_index_digest = Some(active_index_digest(app)?);
            journal.phase = TransactionPhase::RollingBack;
            let mut rollback_errors = save_journal(&journal_path, &journal)
                .err()
                .map(|save_err| {
                    vec![json!({
                        "step": "persist_rolling_back",
                        "message": save_err.message,
                    })]
                })
                .unwrap_or_default();
            if rollback_errors.is_empty() {
                if let Err(validation) = validate_rollback_evidence(app, &plan, &journal) {
                    rollback_errors.push(json!({
                        "step": "validate_rollback_evidence",
                        "message": validation.message,
                    }));
                } else {
                    rollback_errors = rollback_journal(app, &paths, &plan, &journal);
                }
            }
            if rollback_errors.is_empty()
                && std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref()
                    == Some("convergence_interrupt_after_rollback")
            {
                return Err(err);
            }
            if rollback_errors.is_empty() {
                journal.phase = TransactionPhase::RolledBackCleanupPending;
                if let Err(save_err) = save_journal(&journal_path, &journal) {
                    rollback_errors.push(json!({
                        "step": "persist_rolled_back_cleanup_pending",
                        "message": save_err.message,
                    }));
                }
            }
            if rollback_errors.is_empty() {
                rollback_errors.extend(finish_transaction(&journal));
            }
            if rollback_errors.is_empty() {
                cleanup_journal(&journal_path, &journal).map_err(map_io)?;
            }
            return Err(err.with_rollback_errors(rollback_errors));
        }
    };
    journal.result = Some(output.clone());
    journal.phase = TransactionPhase::CommittedCleanupPending;
    save_journal(&journal_path, &journal)?;
    let cleanup_errors = finish_transaction(&journal);
    if !cleanup_errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::IoError,
            "convergence transaction backup cleanup failed",
        )
        .with_rollback_errors(cleanup_errors));
    }
    cleanup_journal(&journal_path, &journal).map_err(map_io)?;
    Ok(apply_output(&plan, cursor, idempotency_key_digest, output))
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
    if journal.source_head.is_none()
        && plan.source.direction == ConvergenceInputDirection::Projection
    {
        journal.phase = TransactionPhase::ReplacingSource;
        save_journal(journal_path, journal)?;
        replace_source_from_projection(app, plan, journal)?;
        maybe_skill_fault("convergence_interrupt_after_source_replacement")?;
        journal.phase = TransactionPhase::SourceReplaced;
        save_journal(journal_path, journal)?;
    }
    let source_commit = if journal.source_head.is_some() {
        journal.source_commit.clone()
    } else {
        source_commit::commit_convergence_source(app, plan, journal_path, journal)?
    };

    let mut projections = snapshot.projections.clone();
    let mut applied = Vec::new();
    journal.phase = TransactionPhase::InstallingProjections;
    save_journal(journal_path, journal)?;
    for (effect, artifact) in plan.projections.iter().zip(&journal.projections) {
        validate_projection_staging_fingerprint(artifact)?;
        let output = execute_prepared_convergence_projection(
            &app.ctx,
            paths,
            snapshot,
            projection_input(snapshot, plan, effect, request_id)?,
            PathBuf::from(&artifact.staging_path),
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
        journal.installed_projections += 1;
        save_journal(journal_path, journal)?;
        maybe_skill_fault("convergence_after_projection_swap")?;
        maybe_skill_fault("convergence_interrupt_after_projection_swap")?;
    }
    journal.expected_projections = Some(projections.clone());
    journal.phase = TransactionPhase::ProjectionsSwapped;
    save_journal(journal_path, journal)?;
    paths
        .save_projections(&projections)
        .map_err(map_registry_state)?;
    maybe_skill_fault("convergence_after_registry_save")?;
    journal.phase = TransactionPhase::CommittingRegistry;
    save_journal(journal_path, journal)?;
    let registry_commit = gitops::commit_paths_if_changed(
        &app.ctx,
        &["state/registry/projections.json"],
        &format!("skill({}): record convergence projections", plan.skill),
    )
    .map_err(map_git)?;
    maybe_skill_fault("convergence_interrupt_committing_registry")?;
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
    for args in [
        ["diff", "--quiet", "--", "state/registry/projections.json"],
        [
            "diff",
            "--cached",
            "--quiet",
            "state/registry/projections.json",
        ],
    ] {
        let output = gitops::run_git_allow_failure(&app.ctx, &args).map_err(map_git)?;
        if !output.status.success() {
            return Err(stale(
                "registry projections changed after planning or are not committed",
                "PLAN_CHECKPOINT_DRIFT",
            ));
        }
    }
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    if plan.registry.initialized != snapshot.is_some() {
        return Err(stale(
            "registry initialization changed after planning",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    if let Some(snapshot) = snapshot {
        let digest = digest_value(&snapshot.checkpoint)?;
        let projections_digest = digest_value(&snapshot.projections)?;
        if plan.registry.checkpoint_digest.as_deref() != Some(digest.as_str())
            || plan.registry.checkpoint_updated_at.as_deref()
                != Some(snapshot.checkpoint.updated_at.to_rfc3339().as_str())
            || plan.registry.projections_digest.as_deref() != Some(projections_digest.as_str())
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
            validate_projection_guard(app, plan, effect)?;
        }
    }
    Ok(())
}

fn replace_source_from_projection(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let source = app.ctx.skill_path(&plan.skill);
    let staging = journal
        .source_staging
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "source staging path is absent")
        })?;
    exchange_paths_atomic(&staging, &source).map_err(map_io)?;
    Ok(())
}

fn save_journal(
    path: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string_pretty(journal).map_err(map_io)?;
    write_atomic(path, &(raw + "\n")).map_err(map_io)
}

fn cleanup_journal(path: &Path, journal: &TransactionJournal) -> std::io::Result<()> {
    remove_path_if_exists(Path::new(&journal.index_backup))?;
    remove_path_if_exists(path)
}

fn finish_committed_cleanup(
    journal_path: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let errors = finish_transaction(journal);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::IoError,
            "committed convergence cleanup is incomplete",
        )
        .with_rollback_errors(errors));
    }
    cleanup_journal(journal_path, journal).map_err(map_io)
}

fn declared_backup(
    path: &Path,
    backup_path: &Path,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(map_io(err)),
    };
    let kind = if metadata.file_type().is_symlink() {
        "symlink"
    } else if metadata.is_dir() {
        "dir"
    } else {
        "file"
    };
    Ok(Some(json!({
        "kind": kind,
        "original_path": path.display().to_string(),
        "backup_path": backup_path.display().to_string(),
    })))
}

fn prepare_transaction_artifacts(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    reserve_owned_dir(
        Path::new(&journal.artifact_root),
        &journal.plan_id,
        &journal.artifact_owner_proof,
    )?;
    gitops::snapshot_index_to(&app.ctx, Path::new(&journal.index_backup)).map_err(map_git)?;
    journal.index_backup_digest = Some(file_digest(Path::new(&journal.index_backup))?);
    save_journal(journal_path, journal)?;
    if let Some(backup) = journal.source_backup.as_ref() {
        create_declared_path_backup(&app.ctx.skill_path(&plan.skill), backup).map_err(map_io)?;
    }
    for projection in &journal.projections {
        if let Some(backup) = projection.backup.as_ref() {
            create_declared_path_backup(Path::new(&projection.materialized_path), backup)
                .map_err(map_io)?;
        }
    }
    maybe_skill_fault("convergence_during_backup_preparation")?;
    let selected_source = selected_source_path(app, plan)?;
    if let Some(staging) = journal.source_staging.as_deref() {
        reserve_owned_dir(
            Path::new(staging).parent().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "source stage has no owner")
            })?,
            &journal.plan_id,
            journal.source_owner_proof.as_deref().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "source owner proof is absent")
            })?,
        )?;
        project_skill_to_target(&selected_source, Path::new(staging), ProjectionMethod::Copy)
            .map_err(map_io)?;
        journal.source_activated_fingerprint =
            Some(convergence_projection_fingerprint(Path::new(staging))?);
    }
    prepare_projection_stages_from(app, plan, "", journal, &selected_source)?;
    Ok(())
}

fn prepare_projection_stages(
    app: &App,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    prepare_projection_stages_from(
        app,
        plan,
        request_id,
        journal,
        &app.ctx.skill_path(&plan.skill),
    )?;
    Ok(())
}

fn prepare_projection_stages_from(
    app: &App,
    plan: &SkillConvergencePlan,
    request_id: &str,
    journal: &mut TransactionJournal,
    source: &Path,
) -> std::result::Result<(), CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
    for (effect, artifact) in plan.projections.iter().zip(journal.projections.iter_mut()) {
        reserve_owned_dir(
            Path::new(&artifact.staging_owner),
            &journal.plan_id,
            &artifact.owner_proof,
        )?;
        let input = projection_input(&snapshot, plan, effect, request_id)?;
        prepare_convergence_projection(
            &app.ctx,
            &input,
            source,
            Path::new(&artifact.staging_path),
        )?;
        artifact.activated_fingerprint = Some(convergence_projection_fingerprint(Path::new(
            &artifact.staging_path,
        ))?);
    }
    Ok(())
}

fn projection_input(
    snapshot: &crate::state_model::RegistrySnapshot,
    plan: &SkillConvergencePlan,
    effect: &crate::core::convergence::ProjectionEffectPlan,
    request_id: &str,
) -> std::result::Result<ProjectionExecutionInput, CommandFailure> {
    let binding = snapshot
        .binding(&effect.binding_id)
        .cloned()
        .ok_or_else(|| stale("planned binding no longer exists", "PLAN_BINDING_DRIFT"))?;
    let target = snapshot
        .target(&effect.target_id)
        .cloned()
        .ok_or_else(|| stale("planned target no longer exists", "PLAN_TARGET_DRIFT"))?;
    Ok(ProjectionExecutionInput {
        context: ProjectionExecutionContext::Convergence,
        skill: plan.skill.clone(),
        binding,
        binding_is_new: false,
        target,
        target_is_new: false,
        source_path: None,
        staging_path: None,
        materialized_path: PathBuf::from(&effect.materialized_path),
        method: parse_method(&effect.method)?,
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
    })
}

fn selected_source_path(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<PathBuf, CommandFailure> {
    if plan.source.direction == ConvergenceInputDirection::Source {
        return Ok(app.ctx.skill_path(&plan.skill));
    }
    let instance = plan.source.input_instance.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "projection input has no instance id",
        )
    })?;
    plan.projections
        .iter()
        .find(|effect| effect.instance_id == instance)
        .map(|effect| PathBuf::from(&effect.materialized_path))
        .ok_or_else(|| CommandFailure::new(ErrorCode::StateCorrupt, "projection input is absent"))
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

fn new_owner_proof(plan_id: &str) -> String {
    format!("{plan_id}:{}", uuid::Uuid::new_v4())
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
