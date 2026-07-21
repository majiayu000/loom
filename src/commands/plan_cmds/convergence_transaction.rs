use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::ProjectionMethod;
use crate::core::convergence::{ConvergenceInputDirection, SkillConvergencePlan};
use crate::fs_util::{exchange_paths_atomic, rename_no_replace_atomic, write_atomic};
use crate::gitops;
use crate::state_model::{RegistryProjectionsFile, RegistryStatePaths, empty_projections_file};
use crate::types::ErrorCode;

use super::super::codex_visibility::projection_path_is_safe_symlink;
use super::super::file_ops::{create_declared_path_backup, restore_path_from_backup_if_absent};
use super::super::helpers::{map_git, map_io, map_lock, map_registry_state};
use super::super::projection_executor::{
    PreparedProjectionStaging, ProjectionExecutionContext, ProjectionExecutionInput,
    convergence_projection_fingerprint, execute_prepared_convergence_projection,
    finish_convergence_projection, prepare_convergence_projection,
};
use super::super::projections::upsert_projection;
use super::super::provenance::{materialized_tree_digest, skill_tree_digest};
use super::super::skill_cmds::shared::{maybe_skill_fault, push_rollback_error};
use super::super::{App, CommandFailure};
use super::converge::digest_value;
use super::{PLAN_PROTOCOL_VERSION, plan_failure};

mod aggregate_record;
mod external_head;
mod faults;
mod guards;
mod index_lock_failure;
mod journal_types;
mod ownership;
mod ownership_state;
mod preparation;
mod projection_recovery;
mod projection_view;
mod recovery_evidence;
mod recovery_support;
mod registry_commit;
mod registry_restore;
mod rollback;
mod source_commit;
mod source_recovery;
use faults::interruption_fault_active;
use guards::{validate_guards, validate_recovery_routing};
#[cfg(test)]
use ownership::reserve_owned_dir;
use ownership::{
    activate_owned_dir, cleanup_owned_dir, owner_proof_is_valid, ownership_attempts_match_journal,
    retain_declared_attempts, validate_owned_staging, validate_transaction_artifacts,
};
use ownership_state::{
    allocate_attempt, archive_previous_terminal_journal, archive_rolled_back_journal,
};
use preparation::{
    declared_backup, prepare_projection_stages, prepare_transaction_artifacts,
    rotate_projection_stages,
};
use projection_recovery::{
    prepare_projection_restore_fingerprint, restore_projection_from_evidence,
    validate_projection_staging_fingerprint,
};
use projection_view::projection_view_digest;
use recovery_evidence::{
    active_index_digest, file_digest, validate_mutated_surfaces, validate_rollback_evidence,
};
use recovery_support::*;
use registry_commit::{commit_convergence_registry, require_head};
use rollback::{finish_transaction, restore_activated_projections, rollback_journal};
use source_recovery::{
    restore_source_after_activation_guard, restore_source_from_evidence,
    validate_activated_source_fingerprint, validate_source_staging_fingerprint,
};

const SCHEMA_VERSION: &str = "1.3";

use journal_types::{ProjectionBackup, TransactionJournal, TransactionPhase};

/// Per-invocation identity and tracing carried through one convergence apply.
pub(super) struct ApplyInvocation<'a> {
    pub identity: &'a super::ConvergenceApplyIdentity,
    pub request_id: &'a str,
}
pub(super) fn apply_convergence(
    app: &App,
    stored: &Value,
    cursor: usize,
    identity: &super::ConvergenceApplyIdentity,
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
    let invocation = ApplyInvocation {
        identity,
        request_id,
    };
    let _workspace_lock = app.ctx.lock_workspace().map_err(map_lock)?;
    let _skill_lock = app.ctx.lock_skill(&plan.skill).map_err(map_lock)?;
    let journal_path = journal_path(app, &plan.skill);
    archive_previous_terminal_journal(app, &journal_path, &plan)?;
    if journal_path.exists()
        && let Some(output) = recover_journal(app, &journal_path, &plan, &invocation)?
    {
        return Ok(apply_output(&plan, cursor, identity, output));
    }
    let snapshot = validate_guards(app, &plan, cursor)?;
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    if snapshot.is_none() && (!plan.projections.is_empty() || plan.registry.initialized) {
        return Err(stale(
            "registry state disappeared after planning",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    let tx_dir = journal_path.parent().expect("journal has parent");
    fs::create_dir_all(tx_dir).map_err(map_io)?;
    let generation = uuid::Uuid::new_v4().hyphenated().to_string();
    let artifact_dir = tx_dir.join(format!("{}-artifacts-{generation}", plan.plan_id));
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
                    ".loom-convergence-source-stage-{}-{generation}.owner/stage",
                    plan.plan_id,
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
                ".loom-projection-stage-{}-{index}-{generation}.owner",
                plan.plan_id,
            ));
            let backup = preparation::declared_projection_backup(
                materialized,
                &artifact_dir.join(format!("projection-{index}")),
            )?;
            let backup = match (effect.effect.as_str(), backup) {
                ("create", None) => None,
                ("refresh", Some(mut backup)) => {
                    backup["view"] = json!(effect.method);
                    Some(backup)
                }
                _ => {
                    return Err(stale(
                        "projection path changed before transaction declaration",
                        "PLAN_PROJECTION_DRIFT",
                    ));
                }
            };
            Ok(ProjectionBackup {
                materialized_path: effect.materialized_path.clone(),
                backup,
                staging_path: staging_owner.join("stage").display().to_string(),
                staging_owner: staging_owner.display().to_string(),
                owner_proof: new_owner_proof(&plan.plan_id),
                activated_fingerprint: None,
                activated: false,
                activation_pending: false,
                original_fingerprint: None,
                restored_fingerprint: None,
                restore_pending: false,
            })
        })
        .collect::<std::result::Result<Vec<_>, CommandFailure>>()?;
    let artifact_owner_proof = new_owner_proof(&plan.plan_id);
    let mut ownership_attempts = vec![allocate_attempt(
        &artifact_dir,
        &plan.plan_id,
        &artifact_owner_proof,
    )?];
    if let (Some(staging), Some(proof)) = (&source_staging, &source_owner_proof) {
        let owner = Path::new(staging).parent().ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "source stage has no owner")
        })?;
        ownership_attempts.push(allocate_attempt(owner, &plan.plan_id, proof)?);
    }
    for projection in &projection_backups {
        ownership_attempts.push(allocate_attempt(
            Path::new(&projection.staging_owner),
            &plan.plan_id,
            &projection.owner_proof,
        )?);
    }
    let mut journal = TransactionJournal {
        plan_id: plan.plan_id.clone(),
        skill: plan.skill.clone(),
        previous_head: plan.source.registry_head.clone(),
        artifact_root: artifact_dir.display().to_string(),
        artifact_owner_proof,
        ownership_attempts,
        index_backup: durable_index.display().to_string(),
        index_backup_digest: None,
        source_backup,
        source_staging,
        source_owner_proof,
        source_activated_fingerprint: None,
        projections: projection_backups,
        original_projections: snapshot
            .as_ref()
            .map_or_else(empty_projections_file, |snapshot| {
                snapshot.projections.clone()
            }),
        installed_projections: 0,
        expected_projections: None,
        source_head: None,
        source_commit: None,
        source_staged_index_digest: None,
        source_index_changed: None,
        registry_commit: None,
        registry_staged_index_digest: None,
        registry_index_attempts: Vec::new(),
        rollback_head: None,
        rollback_index_digest: None,
        preparation_aborted: false,
        result: None,
        phase: TransactionPhase::Preparing,
    };
    save_journal(&journal_path, &journal)?;

    if let Err(err) = prepare_transaction_artifacts(app, &plan, &journal_path, &mut journal) {
        if interruption_fault_active() {
            return Err(err);
        }
        let cleanup_errors = cleanup_declared_artifacts(app, &journal_path, &mut journal, true);
        return Err(err.with_rollback_errors(cleanup_errors));
    }
    journal.phase = TransactionPhase::Prepared;
    save_journal(&journal_path, &journal)?;
    maybe_skill_fault("convergence_interrupt_after_prepared")?;

    let result = execute_local_transaction(
        app,
        &paths,
        snapshot.as_ref(),
        &plan,
        &invocation,
        &journal_path,
        &mut journal,
    );
    let output = match result {
        Ok(output) => output,
        Err(err) if interruption_fault_active() => {
            return Err(err);
        }
        Err(err) => {
            return Err(rollback::handle_transaction_failure(
                app,
                &paths,
                &plan,
                &journal_path,
                &mut journal,
                err,
            )?);
        }
    };
    journal.result = Some(output.clone());
    journal.phase = TransactionPhase::CommittedCleanupPending;
    save_journal(&journal_path, &journal)?;
    let cleanup_errors = finish_transaction(&journal_path, &mut journal);
    if !cleanup_errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::IoError,
            "convergence transaction backup cleanup failed",
        )
        .with_rollback_errors(cleanup_errors));
    }
    Ok(apply_output(&plan, cursor, identity, output))
}

fn execute_local_transaction(
    app: &App,
    paths: &RegistryStatePaths,
    snapshot: Option<&crate::state_model::RegistrySnapshot>,
    plan: &SkillConvergencePlan,
    invocation: &ApplyInvocation<'_>,
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

    let mut projections = snapshot.map_or_else(empty_projections_file, |snapshot| {
        snapshot.projections.clone()
    });
    let mut applied = Vec::new();
    journal.phase = TransactionPhase::InstallingProjections;
    save_journal(journal_path, journal)?;
    for index in 0..plan.projections.len() {
        let effect = &plan.projections[index];
        let live_path = PathBuf::from(&journal.projections[index].materialized_path);
        let safe_symlink_noop = effect.effect == "refresh"
            && effect.method == "symlink"
            && projection_path_is_safe_symlink(&live_path, &app.ctx.skill_path(&plan.skill));
        if effect.method == "symlink" && !safe_symlink_noop {
            let artifact = &mut journal.projections[index];
            let staging_path = Path::new(&artifact.staging_path);
            validate_owned_staging(
                &live_path,
                staging_path,
                &journal.plan_id,
                &artifact.owner_proof,
            )?;
            if !projection_path_is_safe_symlink(staging_path, &app.ctx.skill_path(&plan.skill)) {
                return Err(stale(
                    "prepared symlink does not target the final canonical source",
                    "PLAN_PROJECTION_DRIFT",
                ));
            }
            artifact.activated_fingerprint =
                Some(convergence_projection_fingerprint(staging_path)?);
            save_journal(journal_path, journal)?;
        }
        if !safe_symlink_noop {
            rollback::persist_projection_activation_intent(journal_path, journal, index)?;
        }
        let artifact = &journal.projections[index];
        let snapshot = snapshot.ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "projection transaction has no registry snapshot",
            )
        })?;
        let source_head = journal.source_head.as_deref().ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "journal is missing source head")
        })?;
        require_head(
            app,
            source_head,
            "HEAD changed before projection activation",
        )?;
        source_commit::validate_live_source(app, plan)?;
        let input = projection_input(snapshot, plan, effect, invocation.request_id)?;
        let output = if safe_symlink_noop {
            execute_prepared_convergence_projection(
                &app.ctx,
                paths,
                snapshot,
                input,
                None::<PreparedProjectionStaging>,
                |_| {
                    Err(CommandFailure::new(
                        ErrorCode::StateCorrupt,
                        "safe symlink no-op unexpectedly requested a staging owner",
                    ))
                },
            )?
        } else {
            validate_projection_staging_fingerprint(artifact)?;
            let staging = PreparedProjectionStaging::new(
                PathBuf::from(&artifact.staging_path),
                artifact.fingerprint().map(str::to_string).ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::StateCorrupt,
                        "prepared projection staging fingerprint is absent",
                    )
                })?,
                artifact.original_fingerprint.clone(),
            );
            execute_prepared_convergence_projection(
                &app.ctx,
                paths,
                snapshot,
                input,
                staging,
                |staging_path| {
                    validate_owned_staging(
                        &live_path,
                        staging_path,
                        &journal.plan_id,
                        &artifact.owner_proof,
                    )
                },
            )?
        };
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
        debug_assert_eq!(output.activated, !safe_symlink_noop);
        if output.activated {
            journal.projections[index].activation_pending = false;
            journal.projections[index].mark_activated(true);
            journal.installed_projections += 1;
        }
        if let Err(error) = require_head(
            app,
            source_head,
            "HEAD changed during projection activation",
        ) {
            return Err(
                error.with_rollback_errors(restore_activated_projections(journal_path, journal))
            );
        }
        save_journal(journal_path, journal)?;
        maybe_skill_fault("convergence_after_projection_swap")?;
        maybe_skill_fault("convergence_interrupt_after_projection_swap")?;
    }
    maybe_skill_fault("convergence_interrupt_after_all_projection_swaps")?;
    journal.expected_projections = Some(projections.clone());
    journal.phase = TransactionPhase::ProjectionsSwapped;
    save_journal(journal_path, journal)?;
    if snapshot.is_some() {
        let save_guard = require_head(
            app,
            journal.source_head.as_deref().unwrap_or_default(),
            "HEAD changed before saving projection results",
        )
        .and_then(|_| validate_recovery_routing(app, plan))
        .and_then(|_| validate_mutated_surfaces(app, paths, plan, journal));
        if let Err(error) = save_guard {
            return Err(
                error.with_rollback_errors(restore_activated_projections(journal_path, journal))
            );
        }
        #[cfg(debug_assertions)]
        if let Some(milliseconds) = std::env::var("LOOM_TEST_CONVERGENCE_REGISTRY_SAVE_PAUSE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(milliseconds.min(2_000)));
        }
        let replaced = paths
            .compare_exchange_projections(&journal.original_projections, &projections)
            .map_err(map_registry_state)?;
        if !replaced {
            return Err(stale(
                "registry projections changed during atomic replacement",
                "PLAN_PROJECTION_DRIFT",
            )
            .with_rollback_errors(restore_activated_projections(journal_path, journal)));
        }
        maybe_skill_fault("convergence_interrupt_after_registry_save_cas")?;
        if let Err(error) = require_head(
            app,
            journal.source_head.as_deref().unwrap_or_default(),
            "HEAD changed while saving projection results",
        ) {
            return Err(external_head::handle_external_registry_failure(
                app,
                paths,
                plan,
                journal_path,
                journal,
                error,
            )?);
        }
    }
    maybe_skill_fault("convergence_after_registry_save")?;
    journal.phase = TransactionPhase::CommittingRegistry;
    save_journal(journal_path, journal)?;
    let registry_commit = if snapshot.is_some() {
        match commit_convergence_registry(app, plan, journal_path, journal) {
            Ok(commit) => commit,
            Err(error) => {
                return Err(external_head::handle_external_registry_failure(
                    app,
                    paths,
                    plan,
                    journal_path,
                    journal,
                    error,
                )?);
            }
        }
    } else {
        None
    };
    maybe_skill_fault("convergence_interrupt_committing_registry")?;
    // One aggregate record per convergence, written after every local axis committed so it can
    // never claim more than what actually landed. A source-only plan against an uninitialized
    // registry (B-010) has no operations ledger and apply must not initialize one, so the
    // record is genuinely not applicable there.
    let aggregate_op_id = if snapshot.is_some() {
        Some(aggregate_record::record_convergence_operation(
            paths,
            plan,
            invocation.identity,
            source_commit.as_deref(),
            &applied,
        )?)
    } else {
        None
    };
    Ok(json!({
        "skill": plan.skill,
        "source_commit": source_commit,
        "registry_commit": registry_commit,
        "projection_instances": applied,
        "aggregate_op_id": aggregate_op_id,
    }))
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
    let reviewed = &plan.source.tree_digest;
    if skill_tree_digest(&source).map_err(map_io)? != *reviewed {
        return Err(stale(
            "source changed immediately before projection replacement",
            "source_changed_before_exchange",
        ));
    }
    require_head(
        app,
        &journal.previous_head,
        "HEAD changed before projection source replacement",
    )?;
    validate_source_staging_fingerprint(&source, journal)?;
    exchange_paths_atomic(&staging, &source).map_err(map_io)?;
    if skill_tree_digest(&staging).map_err(map_io)? != *reviewed {
        let mut failure = stale(
            "the displaced source did not match the reviewed source",
            "source_changed_during_exchange",
        );
        if let Err(error) = exchange_paths_atomic(&staging, &source) {
            failure = failure.with_rollback_errors(vec![json!({
                "step": "restore_source_after_exchange_guard_failure",
                "message": error.to_string(),
            })]);
        }
        return Err(failure);
    }
    if let Err(mut failure) = require_head(
        app,
        &journal.previous_head,
        "HEAD changed during projection source replacement",
    ) {
        if let Err(error) = exchange_paths_atomic(&staging, &source) {
            failure = failure.with_rollback_errors(vec![json!({
                "step": "restore_source_after_head_drift",
                "message": error.to_string(),
            })]);
        }
        return Err(failure);
    }
    if let Err(failure) = validate_activated_source_fingerprint(&source, journal) {
        return Err(restore_source_after_activation_guard(
            &source, &staging, reviewed, failure,
        ));
    }
    Ok(())
}

fn save_journal(
    path: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string(journal).map_err(map_io)?;
    write_atomic(path, &raw).map_err(map_io)
}

fn finish_committed_cleanup(
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let errors = finish_transaction(journal_path, journal);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::IoError,
            "committed convergence cleanup is incomplete",
        )
        .with_rollback_errors(errors));
    }
    archive_rolled_back_journal(journal_path, journal)?;
    Ok(())
}

fn projection_input(
    snapshot: &crate::state_model::RegistrySnapshot,
    plan: &SkillConvergencePlan,
    effect: &crate::core::convergence::ProjectionEffectPlan,
    request_id: &str,
) -> std::result::Result<ProjectionExecutionInput, CommandFailure> {
    let replace_existing = match effect.effect.as_str() {
        "create" => false,
        "refresh" => true,
        value => {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("stored projection effect '{value}' is invalid"),
            ));
        }
    };
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
        replace_existing,
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
