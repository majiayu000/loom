use super::recovery_evidence::{
    reprove_source_boundary, rollback_uncommitted_source_only, validate_expected_projections,
    validate_mutated_surfaces, validate_rollback_evidence, validate_rolling_back_state,
};
use super::registry_restore::restore_registry_projections_if_owned;
use super::*;

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
        TransactionPhase::CommittedArtifactsRetained => {
            return super::recovery_evidence::replay_committed_retained(app, plan, &mut journal)
                .map(Some);
        }
        TransactionPhase::RolledBackArtifactsRetained => {
            archive_rolled_back_journal(journal_path, &journal)?;
            return Ok(None);
        }
        TransactionPhase::CommittedCleanupPending => {
            reprove_source_boundary(app, plan, &journal)?;
            let paths = RegistryStatePaths::from_app_context(&app.ctx);
            validate_mutated_surfaces(app, &paths, plan, &mut journal)?;
            let result = journal.result.clone().ok_or_else(|| {
                CommandFailure::new(ErrorCode::StateCorrupt, "committed journal has no result")
            })?;
            finish_committed_cleanup(journal_path, &mut journal)?;
            return Ok(Some(result));
        }
        TransactionPhase::RolledBackCleanupPending => {
            finish_committed_cleanup(journal_path, &mut journal)?;
            return Ok(None);
        }
        TransactionPhase::Preparing | TransactionPhase::Prepared => {
            let snapshot = match super::guards::validate_pre_mutation_recovery_guards(app, plan) {
                Ok(snapshot) => snapshot,
                Err(failure) if failure.code == ErrorCode::DependencyConflict => {
                    return Err(super::external_head::retire_stale_pre_mutation_journal(
                        app,
                        journal_path,
                        &mut journal,
                        failure,
                    )?);
                }
                Err(failure) => return Err(failure),
            };
            if journal.phase == TransactionPhase::Preparing {
                super::preparation::prepare_transaction_artifacts_from_snapshot(
                    app,
                    snapshot.as_ref(),
                    plan,
                    journal_path,
                    &mut journal,
                )?;
                journal.phase = TransactionPhase::Prepared;
                save_journal(journal_path, &journal)?;
            }
            let paths = RegistryStatePaths::from_app_context(&app.ctx);
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
            finish_committed_cleanup(journal_path, &mut journal)?;
            return Ok(Some(result));
        }
        TransactionPhase::CommittingSource => {
            let paths = RegistryStatePaths::from_app_context(&app.ctx);
            if super::recovery_evidence::retire_uncommitted_source_after_external_head(
                app,
                &paths,
                plan,
                journal_path,
                &mut journal,
            )? {
                return Ok(None);
            }
            super::source_commit::recover_source_index_lock_if_owned(app, plan, &journal)?;
            prove_source_boundary(app, plan, &mut journal)?;
        }
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
            super::registry_commit::terminalize_registry_index_attempts(&mut journal, false);
            journal.phase = TransactionPhase::RolledBackCleanupPending;
            save_journal(journal_path, &journal)?;
            finish_committed_cleanup(journal_path, &mut journal)?;
            return Ok(None);
        }
        _ => {}
    }
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    if let Some(result) =
        super::external_head::complete_durable_registry_noop(app, plan, journal_path, &mut journal)?
    {
        return Ok(Some(result));
    }
    if matches!(
        journal.phase,
        TransactionPhase::ProjectionsSwapped | TransactionPhase::CommittingRegistry
    ) && super::external_head::recover_registry_after_external_head(
        app,
        &paths,
        plan,
        journal_path,
        &mut journal,
    )? {
        return Ok(None);
    }
    if matches!(
        journal.phase,
        TransactionPhase::ReplacingSource
            | TransactionPhase::SourceReplaced
            | TransactionPhase::CommittingSource
    ) && !source_is_committed(&journal)
    {
        rollback_uncommitted_source_only(app, plan, &journal)?;
        let errors = cleanup_declared_artifacts(app, journal_path, &mut journal, false);
        return super::source_recovery::finish_uncommitted_source_recovery(
            journal_path,
            &journal,
            errors,
        );
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
            finish_committed_cleanup(journal_path, &mut journal)?;
            return Ok(Some(result));
        }
        validate_mutated_surfaces(app, &paths, plan, &mut journal)?;
        validate_rollback_evidence(app, plan, &journal)?;
        restore_projections_for_resume(&paths, plan, journal_path, &mut journal)?;
        rotate_projection_stages(journal_path, &mut journal)?;
        journal.installed_projections = 0;
        journal.expected_projections = None;
        prepare_projection_stages(app, plan, request_id, journal_path, &mut journal)?;
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
        finish_committed_cleanup(journal_path, &mut journal)?;
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
    super::registry_commit::terminalize_registry_index_attempts(&mut journal, false);
    journal.phase = TransactionPhase::RolledBackCleanupPending;
    save_journal(journal_path, &journal)?;
    finish_committed_cleanup(journal_path, &mut journal)?;
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
    journal_path: &Path,
    journal: &mut TransactionJournal,
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
        && let Err(err) = restore_registry_projections_if_owned(paths, journal)
    {
        push_rollback_error(&mut errors, "restore_registry_projections", err.message);
    }
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "failed to prepare committed source recovery",
        )
        .with_rollback_errors(errors));
    }
    super::rollback::restore_activated_projections_durably(journal_path, journal)?;
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
    app: &App,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    preparation_aborted: bool,
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
    errors.extend(retain_declared_attempts(journal));
    if errors.is_empty()
        && let Err(error) = super::external_head::capture_current_rollback_evidence(app, journal)
    {
        push_rollback_error(
            &mut errors,
            "capture_declared_cleanup_boundary",
            error.message,
        );
    }
    if errors.is_empty() {
        super::registry_commit::terminalize_registry_index_attempts(journal, false);
        journal.preparation_aborted = preparation_aborted;
        journal.phase = TransactionPhase::RolledBackArtifactsRetained;
        if let Err(err) = save_journal(journal_path, journal) {
            push_rollback_error(
                &mut errors,
                "persist_declared_retained_artifacts",
                err.message,
            );
        }
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
    let expected_journal = tx_dir.join(format!("convergence-{}.json", plan.skill));
    let mut valid = journal.plan_id == plan.plan_id
        && journal.skill == plan.skill
        && journal.previous_head == plan.source.registry_head
        && journal_path == expected_journal
        && generated_owned_path_matches(
            Path::new(&journal.artifact_root),
            &tx_dir,
            &format!("{}-artifacts-", plan.plan_id),
            "",
        )
        && owner_proof_is_valid(&plan.plan_id, &journal.artifact_owner_proof)
        && ownership_attempts_match_journal(journal)
        && super::registry_commit::registry_index_attempts_valid(journal)
        && Path::new(&journal.index_backup) == Path::new(&journal.artifact_root).join("index")
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
            journal.source_staging.as_deref().is_some_and(|staging| {
                Path::new(staging).parent().is_some_and(|owner| {
                    generated_owned_path_matches(
                        owner,
                        &app.ctx.skills_dir,
                        &format!(".loom-convergence-source-stage-{}-", plan.plan_id),
                        ".owner",
                    )
                }) && Path::new(staging)
                    .file_name()
                    .is_some_and(|name| name == "stage")
            }) && journal
                .source_owner_proof
                .as_deref()
                .is_some_and(|proof| owner_proof_is_valid(&plan.plan_id, proof))
                && backup_matches(
                    journal.source_backup.as_ref(),
                    &app.ctx.skill_path(&plan.skill),
                    &Path::new(&journal.artifact_root).join("source"),
                )
                && (journal.phase == TransactionPhase::Preparing
                    || super::ownership::is_pre_mutation_retained(journal)
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
        let owner_valid = materialized.parent().is_some_and(|parent| {
            generated_owned_path_matches(
                Path::new(&artifact.staging_owner),
                parent,
                &format!(".loom-projection-stage-{}-{index}-", plan.plan_id),
                ".owner",
            )
        });
        let backup_valid = match effect.effect.as_str() {
            "refresh" => backup_matches(
                artifact.backup.as_ref(),
                materialized,
                &Path::new(&journal.artifact_root).join(format!("projection-{index}")),
            ),
            "create" => artifact.backup.is_none(),
            _ => false,
        };
        valid &= artifact.materialized_path == effect.materialized_path
            && owner_valid
            && Path::new(&artifact.staging_path)
                == Path::new(&artifact.staging_owner).join("stage")
            && owner_proof_is_valid(&plan.plan_id, &artifact.owner_proof)
            && (journal.phase == TransactionPhase::Preparing
                || super::ownership::is_pre_mutation_retained(journal)
                || artifact.fingerprint().is_some_and(valid_sha256_digest))
            && match effect.effect.as_str() {
                "refresh" => {
                    journal.phase == TransactionPhase::Preparing
                        || artifact
                            .backup
                            .as_ref()
                            .and_then(|backup| backup["backup_digest"].as_str())
                            .is_some_and(valid_sha256_digest)
                }
                "create" => artifact.backup.is_none(),
                _ => false,
            }
            && backup_valid;
    }
    valid &= journal.installed_projections
        == journal
            .projections
            .iter()
            .filter(|projection| projection.is_activated())
            .count();
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

pub(super) fn generated_owned_path_matches(
    path: &Path,
    parent: &Path,
    prefix: &str,
    suffix: &str,
) -> bool {
    if path.parent() != Some(parent) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(generation) = name
        .strip_prefix(prefix)
        .and_then(|name| name.strip_suffix(suffix))
    else {
        return false;
    };
    uuid::Uuid::parse_str(generation)
        .ok()
        .is_some_and(|uuid| uuid.hyphenated().to_string() == generation)
}

pub(super) fn validate_phase_invariants(journal: &TransactionJournal) -> bool {
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
        TransactionPhase::RollingBack
            | TransactionPhase::RolledBackCleanupPending
            | TransactionPhase::RolledBackArtifactsRetained
    ) == (journal.rollback_head.is_some()
        && journal.rollback_index_digest.is_some());
    let registry_evidence =
        journal.registry_commit.is_some() == journal.registry_staged_index_digest.is_some();
    let registry_evidence_phase = journal.registry_commit.is_none()
        || matches!(
            journal.phase,
            TransactionPhase::CommittingRegistry
                | TransactionPhase::CommittedCleanupPending
                | TransactionPhase::CommittedArtifactsRetained
        );
    let preparation_abort_phase =
        !journal.preparation_aborted || super::ownership::is_pre_mutation_retained(journal);
    source_relation
        && preparation_abort_phase
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
                    && journal.installed_projections <= count
                    && journal.expected_projections.is_some()
                    && journal.result.is_none()
            }
            TransactionPhase::RollingBack
            | TransactionPhase::RolledBackCleanupPending
            | TransactionPhase::RolledBackArtifactsRetained => journal.result.is_none(),
            TransactionPhase::CommittedCleanupPending
            | TransactionPhase::CommittedArtifactsRetained => {
                journal.source_head.is_some()
                    && journal.installed_projections <= count
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
        super::registry_commit::align_registry_index(app, plan, journal_path, journal, &head)?;
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

pub(super) fn committed_result_with_registry(
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
