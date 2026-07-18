use super::projection_recovery::restore_projection_from_evidence;
use super::recovery_evidence::{
    reprove_source_boundary, rollback_uncommitted_source_only, validate_expected_projections,
    validate_mutated_surfaces, validate_rollback_evidence, validate_rolling_back_state,
};
use super::*;
use std::fs::OpenOptions;
use std::io::Write;

const OWNER_FILE: &str = ".owner";
const RESERVATION_PROOF_FILE: &str = ".reservation-owner";

pub(super) fn interruption_fault_active() -> bool {
    matches!(
        std::env::var("LOOM_FAULT_INJECT").ok().as_deref(),
        Some(
            "convergence_interrupt_after_source_commit"
                | "convergence_interrupt_after_source_cas"
                | "convergence_interrupt_committing_source"
                | "convergence_interrupt_committing_registry"
                | "convergence_interrupt_after_owner_root_creation"
                | "convergence_interrupt_after_owner_marker_write"
                | "convergence_interrupt_after_prepared"
                | "convergence_interrupt_after_source_replacement"
                | "convergence_interrupt_after_source_add"
                | "convergence_interrupt_after_staged_index_prepared"
                | "convergence_interrupt_after_staged_index_install"
                | "convergence_interrupt_after_projection_activation"
                | "convergence_interrupt_after_projection_swap"
                | "convergence_interrupt_before_registry_cas"
        )
    )
}

pub(super) fn reserve_owned_dir(
    path: &Path,
    plan_id: &str,
    reservation_proof: &str,
) -> std::result::Result<(), CommandFailure> {
    if !owner_proof_is_valid(plan_id, reservation_proof) {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "journal owner proof is invalid",
        ));
    }
    let (reservation, staging) = reservation_paths(path, plan_id)?;
    let mut token = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&reservation)
        .map_err(|err| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!(
                    "artifact reservation collision at {}: {err}",
                    path.display()
                ),
            )
        })?;
    writeln!(token, "{reservation_proof}").map_err(map_io)?;
    token.sync_all().map_err(map_io)?;
    if let Err(err) = fs::create_dir(&staging) {
        drop(token);
        let mut cleanup_errors = Vec::new();
        if let Err(cleanup_err) = fs::remove_file(&reservation) {
            push_rollback_error(
                &mut cleanup_errors,
                "remove_failed_artifact_reservation_token",
                cleanup_err,
            );
        }
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("artifact staging collision at {}: {err}", staging.display()),
        )
        .with_rollback_errors(cleanup_errors));
    }
    let proof_path = staging.join(RESERVATION_PROOF_FILE);
    let mut proof = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&proof_path)
        .map_err(map_io)?;
    writeln!(proof, "{reservation_proof}").map_err(map_io)?;
    proof.sync_all().map_err(map_io)?;
    maybe_skill_fault("convergence_interrupt_after_owner_root_creation")?;
    let owner = staging.join(OWNER_FILE);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&owner)
        .map_err(map_io)?;
    writeln!(file, "{plan_id}").map_err(map_io)?;
    file.sync_all().map_err(map_io)?;
    maybe_skill_fault("convergence_interrupt_after_owner_marker_write")?;
    if let Err(err) = rename_no_replace_atomic(&staging, path) {
        let mut cleanup_errors = Vec::new();
        cleanup_reservation(path, plan_id, reservation_proof, &mut cleanup_errors);
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "artifact reservation collision at {}: {err}",
                path.display()
            ),
        )
        .with_rollback_errors(cleanup_errors));
    }
    fs::remove_file(&reservation).map_err(map_io)
}

pub(super) fn recover_journal(
    app: &App,
    journal_path: &Path,
    plan: &SkillConvergencePlan,
    request_id: &str,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let raw = fs::read_to_string(journal_path).map_err(map_io)?;
    let mut journal: TransactionJournal = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("invalid convergence journal: {err}"),
        )
    })?;
    validate_journal(app, journal_path, plan, &journal)?;
    match journal.phase {
        TransactionPhase::CommittedCleanupPending => {
            reprove_source_boundary(app, plan, &journal)?;
            let paths = RegistryStatePaths::from_app_context(&app.ctx);
            validate_mutated_surfaces(app, &paths, plan, &mut journal)?;
            let result = journal.result.clone().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "committed journal has no result")
            })?;
            finish_committed_cleanup(journal_path, &journal)?;
            return Ok(Some(result));
        }
        TransactionPhase::RolledBackCleanupPending => {
            finish_committed_cleanup(journal_path, &journal)?;
            return Ok(None);
        }
        TransactionPhase::Preparing | TransactionPhase::Prepared => {
            let errors = cleanup_declared_artifacts(journal_path, &journal);
            if errors.is_empty() {
                return Ok(None);
            }
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "interrupted preparation cleanup failed",
            )
            .with_rollback_errors(errors));
        }
        TransactionPhase::CommittingSource => prove_source_boundary(app, plan, &mut journal)?,
        TransactionPhase::RollingBack => {
            let paths = RegistryStatePaths::from_app_context(&app.ctx);
            validate_mutated_surfaces(app, &paths, plan, &mut journal)?;
            validate_rollback_evidence(app, plan, &journal)?;
            validate_rolling_back_state(app, plan, &journal)?;
            let errors = rollback_journal(app, &paths, plan, &mut journal);
            if !errors.is_empty() {
                return Err(CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    "interrupted convergence rollback failed",
                )
                .with_rollback_errors(errors));
            }
            journal.phase = TransactionPhase::RolledBackCleanupPending;
            save_journal(journal_path, &journal)?;
            finish_committed_cleanup(journal_path, &journal)?;
            return Ok(None);
        }
        _ => {}
    }
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    if matches!(
        journal.phase,
        TransactionPhase::ReplacingSource
            | TransactionPhase::SourceReplaced
            | TransactionPhase::CommittingSource
    ) && !source_is_committed(&journal)
    {
        rollback_uncommitted_source_only(app, plan, &journal)?;
        let errors = cleanup_declared_artifacts(journal_path, &journal);
        if errors.is_empty() {
            return Ok(None);
        }
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "uncommitted source cleanup failed",
        )
        .with_rollback_errors(errors));
    }
    if source_is_committed(&journal) {
        reprove_source_boundary(app, plan, &journal)?;
        validate_recovery_routing(app, plan)?;
        if journal.phase == TransactionPhase::CommittingRegistry {
            let registry_commit = prove_registry_boundary(app, plan, journal_path, &mut journal)?;
            let result = committed_result_with_registry(plan, &journal, registry_commit);
            journal.result = Some(result.clone());
            journal.phase = TransactionPhase::CommittedCleanupPending;
            save_journal(journal_path, &journal)?;
            finish_committed_cleanup(journal_path, &journal)?;
            return Ok(Some(result));
        }
        validate_mutated_surfaces(app, &paths, plan, &mut journal)?;
        validate_rollback_evidence(app, plan, &journal)?;
        restore_projections_for_resume(&paths, plan, &journal)?;
        journal.installed_projections = 0;
        journal.expected_projections = None;
        prepare_projection_stages(app, plan, request_id, &mut journal)?;
        journal.phase = TransactionPhase::SourceCommitted;
        save_journal(journal_path, &journal)?;
        let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
        let result = execute_local_transaction(
            app,
            &paths,
            snapshot.as_ref(),
            plan,
            request_id,
            journal_path,
            &mut journal,
        )?;
        journal.result = Some(result.clone());
        journal.phase = TransactionPhase::CommittedCleanupPending;
        save_journal(journal_path, &journal)?;
        finish_committed_cleanup(journal_path, &journal)?;
        return Ok(Some(result));
    }
    validate_rollback_evidence(app, plan, &journal)?;
    let errors = rollback_journal(app, &paths, plan, &mut journal);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "interrupted convergence recovery failed",
        )
        .with_rollback_errors(errors));
    }
    journal.phase = TransactionPhase::RolledBackCleanupPending;
    save_journal(journal_path, &journal)?;
    finish_committed_cleanup(journal_path, &journal)?;
    Ok(None)
}

pub(super) fn validate_projection_guard(
    app: &App,
    plan: &SkillConvergencePlan,
    effect: &crate::core::convergence::ProjectionEffectPlan,
) -> std::result::Result<(), CommandFailure> {
    let path = Path::new(&effect.materialized_path);
    if let Some(expected) = effect.materialized_tree_digest.as_deref() {
        let live = projection_view_digest(path, &effect.method)?;
        if live == expected {
            return Ok(());
        }
    } else {
        match fs::symlink_metadata(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound && effect.effect == "create" => {
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Ok(metadata)
                if effect.effect == "refresh"
                    && effect.method == "symlink"
                    && metadata.file_type().is_symlink()
                    && projection_path_is_safe_symlink(path, &app.ctx.skill_path(&plan.skill)) =>
            {
                return Ok(());
            }
            Err(err) => return Err(map_io(err)),
            Ok(_) => {}
        }
    }
    Err(stale(
        "projection bytes or path kind changed after planning",
        "PLAN_PROJECTION_DRIFT",
    ))
}

pub(super) fn apply_output(
    plan: &SkillConvergencePlan,
    cursor: usize,
    key_digest: &str,
    output: Value,
) -> Value {
    json!({
        "protocol_version": PLAN_PROTOCOL_VERSION,
        "schema_version": SCHEMA_VERSION,
        "plan_id": plan.plan_id,
        "idempotency_key_digest": key_digest,
        "idempotent_replay": false,
        "plan_event_cursor": cursor,
        "applied": output,
        "recovery": { "rollback_supported": true },
    })
}

pub(super) fn source_is_committed(journal: &TransactionJournal) -> bool {
    journal.source_head.is_some()
}

pub(super) fn restore_projections_for_resume(
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let mut errors = validate_transaction_artifacts(journal);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "committed source recovery artifact validation failed",
        )
        .with_rollback_errors(errors));
    }
    if plan.registry.initialized
        && let Err(err) = paths.save_projections(&journal.original_projections)
    {
        push_rollback_error(&mut errors, "restore_registry_projections", err);
    }
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "failed to prepare committed source recovery",
        )
        .with_rollback_errors(errors));
    }
    for projection in journal
        .projections
        .iter()
        .take(journal.installed_projections)
        .rev()
    {
        if let Err(err) = restore_projection_from_evidence(projection, &journal.plan_id) {
            push_rollback_error(&mut errors, "restore_projection_from_evidence", err.message);
        }
        if !errors.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "failed to prepare committed source recovery",
            )
            .with_rollback_errors(errors));
        }
    }
    for projection in journal.projections.iter().rev() {
        cleanup_owned_dir(
            Path::new(&projection.staging_owner),
            &journal.plan_id,
            &projection.owner_proof,
            &mut errors,
        );
        if !errors.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "failed to prepare committed source recovery",
            )
            .with_rollback_errors(errors));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "failed to prepare committed source recovery",
        )
        .with_rollback_errors(errors))
    }
}

pub(super) fn cleanup_declared_artifacts(
    journal_path: &Path,
    journal: &TransactionJournal,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if std::env::var("LOOM_CLEANUP_FAULT_INJECT").ok().as_deref()
        == Some("convergence_fail_declared_cleanup")
    {
        push_rollback_error(
            &mut errors,
            "cleanup_declared_transaction_artifacts",
            "fault injected before declared artifact cleanup",
        );
        return errors;
    }
    errors = validate_transaction_artifacts(journal);
    if !errors.is_empty() {
        return errors;
    }
    for projection in journal.projections.iter().rev() {
        cleanup_owned_dir(
            Path::new(&projection.staging_owner),
            &journal.plan_id,
            &projection.owner_proof,
            &mut errors,
        );
        if !errors.is_empty() {
            return errors;
        }
    }
    if let Some(path) = journal.source_staging.as_deref()
        && let Some(owner) = Path::new(path).parent()
        && let Some(proof) = journal.source_owner_proof.as_deref()
    {
        cleanup_owned_dir(owner, &journal.plan_id, proof, &mut errors);
        if !errors.is_empty() {
            return errors;
        }
    }
    cleanup_owned_dir(
        Path::new(&journal.artifact_root),
        &journal.plan_id,
        &journal.artifact_owner_proof,
        &mut errors,
    );
    if errors.is_empty()
        && let Err(err) = remove_path_if_exists(journal_path)
    {
        push_rollback_error(&mut errors, "remove_transaction_journal", err);
    }
    errors
}

fn validate_journal(
    app: &App,
    journal_path: &Path,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let tx_dir = app.ctx.state_dir.join("transactions");
    let artifact_root = tx_dir.join(format!("{}-artifacts", plan.plan_id));
    let expected_journal = tx_dir.join(format!("convergence-{}.json", plan.skill));
    let source_stage = app.ctx.skills_dir.join(format!(
        ".loom-convergence-source-stage-{}.owner/stage",
        plan.plan_id
    ));
    let mut valid = journal.plan_id == plan.plan_id
        && journal.skill == plan.skill
        && journal.previous_head == plan.source.registry_head
        && journal_path == expected_journal
        && Path::new(&journal.artifact_root) == artifact_root
        && owner_proof_is_valid(&plan.plan_id, &journal.artifact_owner_proof)
        && Path::new(&journal.index_backup) == artifact_root.join("index")
        && journal.projections.len() == plan.projections.len();
    let previous_projections = gitops::run_git(
        &app.ctx,
        &[
            "show",
            &format!("{}:state/registry/projections.json", journal.previous_head),
        ],
    )
    .ok()
    .and_then(|raw| serde_json::from_str::<RegistryProjectionsFile>(&raw).ok());
    valid &= if plan.registry.initialized {
        previous_projections.as_ref().is_some_and(|previous| {
            serde_json::to_value(previous).ok()
                == serde_json::to_value(&journal.original_projections).ok()
        })
    } else {
        previous_projections.is_none()
            && journal.original_projections.projections.is_empty()
            && journal.original_projections.schema_version
                == crate::state_model::REGISTRY_SCHEMA_VERSION
    };
    valid &= match plan.source.direction {
        ConvergenceInputDirection::Source => {
            journal.source_backup.is_none()
                && journal.source_staging.is_none()
                && journal.source_owner_proof.is_none()
                && journal.source_activated_fingerprint.is_none()
        }
        ConvergenceInputDirection::Projection => {
            journal.source_staging.as_deref() == source_stage.to_str()
                && journal
                    .source_owner_proof
                    .as_deref()
                    .is_some_and(|proof| owner_proof_is_valid(&plan.plan_id, proof))
                && backup_matches(
                    journal.source_backup.as_ref(),
                    &app.ctx.skill_path(&plan.skill),
                    &artifact_root.join("source"),
                )
                && (journal.phase == TransactionPhase::Preparing
                    || journal
                        .source_activated_fingerprint
                        .as_deref()
                        .is_some_and(valid_sha256_digest))
        }
    };
    for (index, (effect, artifact)) in plan
        .projections
        .iter()
        .zip(&journal.projections)
        .enumerate()
    {
        let materialized = Path::new(&effect.materialized_path);
        let owner = materialized.parent().unwrap_or(Path::new("")).join(format!(
            ".loom-projection-stage-{}-{index}.owner",
            plan.plan_id
        ));
        let backup_valid = match effect.effect.as_str() {
            "refresh" => backup_matches(
                artifact.backup.as_ref(),
                materialized,
                &artifact_root.join(format!("projection-{index}")),
            ),
            "create" => artifact.backup.is_none(),
            _ => false,
        };
        valid &= artifact.materialized_path == effect.materialized_path
            && Path::new(&artifact.staging_owner) == owner
            && Path::new(&artifact.staging_path) == owner.join("stage")
            && owner_proof_is_valid(&plan.plan_id, &artifact.owner_proof)
            && (journal.phase == TransactionPhase::Preparing
                || artifact
                    .activated_fingerprint
                    .as_deref()
                    .is_some_and(valid_sha256_digest))
            && backup_valid;
    }
    valid &= validate_phase_invariants(journal);
    valid &= validate_expected_projections(plan, journal);
    if valid {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "convergence journal does not match the reviewed plan",
        ))
    }
}

fn valid_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_phase_invariants(journal: &TransactionJournal) -> bool {
    let count = journal.projections.len();
    let source_relation = journal
        .source_commit
        .as_ref()
        .is_none_or(|commit| journal.source_head.as_ref() == Some(commit));
    let pre_source = journal.source_head.is_none()
        && journal.source_commit.is_none()
        && journal.installed_projections == 0
        && journal.expected_projections.is_none()
        && journal.result.is_none();
    let source_only = journal.source_head.is_some()
        && journal.installed_projections == 0
        && journal.expected_projections.is_none()
        && journal.result.is_none();
    let index_evidence =
        journal.phase == TransactionPhase::Preparing || journal.index_backup_digest.is_some();
    let rollback_evidence = matches!(
        journal.phase,
        TransactionPhase::RollingBack | TransactionPhase::RolledBackCleanupPending
    ) == (journal.rollback_head.is_some()
        && journal.rollback_index_digest.is_some());
    let registry_evidence =
        journal.registry_commit.is_some() == journal.registry_staged_index_digest.is_some();
    let registry_evidence_phase = journal.registry_commit.is_none()
        || matches!(
            journal.phase,
            TransactionPhase::CommittingRegistry | TransactionPhase::CommittedCleanupPending
        );
    source_relation
        && index_evidence
        && rollback_evidence
        && registry_evidence
        && registry_evidence_phase
        && journal.installed_projections <= count
        && match journal.phase {
            TransactionPhase::Preparing
            | TransactionPhase::Prepared
            | TransactionPhase::ReplacingSource
            | TransactionPhase::SourceReplaced
            | TransactionPhase::CommittingSource => pre_source,
            TransactionPhase::SourceCommitted => source_only,
            TransactionPhase::InstallingProjections => {
                journal.source_head.is_some()
                    && journal.expected_projections.is_none()
                    && journal.result.is_none()
            }
            TransactionPhase::ProjectionsSwapped | TransactionPhase::CommittingRegistry => {
                journal.source_head.is_some()
                    && journal.installed_projections == count
                    && journal.expected_projections.is_some()
                    && journal.result.is_none()
            }
            TransactionPhase::RollingBack | TransactionPhase::RolledBackCleanupPending => {
                journal.result.is_none()
            }
            TransactionPhase::CommittedCleanupPending => {
                journal.source_head.is_some()
                    && journal.installed_projections == count
                    && journal.expected_projections.is_some()
                    && journal.result.is_some()
            }
        }
}

fn backup_matches(backup: Option<&Value>, original: &Path, stored: &Path) -> bool {
    backup.is_some_and(|backup| {
        backup["original_path"].as_str() == original.to_str()
            && backup["backup_path"].as_str() == stored.to_str()
            && matches!(backup["kind"].as_str(), Some("dir" | "file" | "symlink"))
    })
}

fn prove_source_boundary(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    let digest = skill_tree_digest(&app.ctx.skill_path(&plan.skill)).map_err(map_io)?;
    if digest != plan.input.selected_input_tree_digest {
        return Err(recovery_stale(
            "source tree does not match the selected convergence input",
        ));
    }
    if head == journal.previous_head {
        let rel = format!("skills/{}", plan.skill);
        let unstaged = gitops::run_git_allow_failure(&app.ctx, &["diff", "--quiet", "--", &rel])
            .map_err(map_git)?;
        let staged =
            gitops::run_git_allow_failure(&app.ctx, &["diff", "--cached", "--quiet", "--", &rel])
                .map_err(map_git)?;
        if !unstaged.status.success() || !staged.status.success() {
            return Ok(());
        }
        journal.source_head = Some(head);
        journal.source_commit = None;
    } else {
        verify_commit(
            app,
            &head,
            &journal.previous_head,
            &format!("skill({}): converge source", plan.skill),
            |path| {
                path == format!("skills/{}", plan.skill)
                    || path.starts_with(&format!("skills/{}/", plan.skill))
            },
        )?;
        journal.source_head = Some(head.clone());
        journal.source_commit = Some(head);
    }
    journal.phase = TransactionPhase::SourceCommitted;
    Ok(())
}

fn prove_registry_boundary(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<String>, CommandFailure> {
    let source_head = journal.source_head.as_deref().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "journal is missing source head")
    })?;
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if !plan.registry.initialized {
        if RegistryStatePaths::from_app_context(&app.ctx).exists() || head != source_head {
            return Err(recovery_stale(
                "source-only transaction unexpectedly changed registry state",
            ));
        }
        return Ok(None);
    }
    validate_registry_result(app, plan, journal)?;
    if head == source_head {
        return super::registry_commit::commit_convergence_registry(
            app,
            plan,
            journal_path,
            journal,
        );
    } else {
        verify_commit(
            app,
            &head,
            source_head,
            &format!("skill({}): record convergence projections", plan.skill),
            |path| path == "state/registry/projections.json",
        )?;
        super::registry_commit::align_registry_index(app, plan, journal, &head)?;
    }
    Ok(Some(head))
}

pub(super) fn verify_commit(
    app: &App,
    head: &str,
    expected_parent: &str,
    expected_subject: &str,
    path_allowed: impl Fn(&str) -> bool,
) -> std::result::Result<(), CommandFailure> {
    let parent = gitops::run_git(&app.ctx, &["rev-parse", &format!("{head}^")]).map_err(map_git)?;
    let subject =
        gitops::run_git(&app.ctx, &["log", "-1", "--format=%s", head]).map_err(map_git)?;
    let paths = gitops::run_git(
        &app.ctx,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", head],
    )
    .map_err(map_git)?;
    if parent != expected_parent
        || subject != expected_subject
        || paths.lines().next().is_none()
        || !paths.lines().all(path_allowed)
    {
        return Err(recovery_stale("HEAD is not the transaction-created commit"));
    }
    Ok(())
}

pub(super) fn validate_registry_result(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
    let expected = journal.expected_projections.as_ref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "journal is missing expected projections",
        )
    })?;
    if serde_json::to_value(&snapshot.projections).map_err(map_io)?
        != serde_json::to_value(expected).map_err(map_io)?
    {
        return Err(recovery_stale(
            "live registry differs from transaction evidence",
        ));
    }
    for effect in &plan.projections {
        let projection = snapshot
            .projections
            .projections
            .iter()
            .find(|item| {
                item.instance_id == effect.instance_id
                    && item.materialized_path == effect.materialized_path
            })
            .ok_or_else(|| recovery_stale("registry projection result is missing"))?;
        let digest = effect.source_tree_digest.as_str();
        let materialized_matches = if effect.method == "symlink" {
            projection.materialized_tree_digest.is_none()
                && projection_path_is_safe_symlink(
                    Path::new(&effect.materialized_path),
                    &app.ctx.skill_path(&plan.skill),
                )
        } else {
            projection.materialized_tree_digest.as_deref() == Some(digest)
        };
        let source_matches = if effect.method == "symlink" {
            projection.source_tree_digest.is_none()
        } else {
            projection.source_tree_digest.as_deref() == Some(digest)
        };
        if projection.method.as_str() != effect.method || !source_matches || !materialized_matches {
            return Err(recovery_stale(
                "registry projection evidence does not match the plan",
            ));
        }
    }
    Ok(())
}

fn committed_result_with_registry(
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    registry_commit: Option<String>,
) -> Value {
    json!({
        "skill": plan.skill,
        "source_commit": journal.source_commit,
        "registry_commit": registry_commit,
        "projection_instances": plan.projections.iter().map(|item| item.instance_id.clone()).collect::<Vec<_>>(),
    })
}

pub(super) fn recovery_stale(message: &str) -> CommandFailure {
    plan_failure(
        ErrorCode::DependencyConflict,
        message,
        "PLAN_STALE",
        false,
        vec!["inspect and resolve the interrupted convergence journal".to_string()],
        None,
    )
}
