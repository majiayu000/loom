use super::*;

pub(super) fn retire_stale_pre_mutation_journal(
    app: &App,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    failure: CommandFailure,
) -> std::result::Result<CommandFailure, CommandFailure> {
    capture_current_rollback_evidence(app, journal)?;
    let errors = cleanup_declared_artifacts(app, journal_path, journal, true);
    if errors.is_empty() {
        archive_rolled_back_journal(journal_path, journal)?;
    }
    Ok(failure.with_rollback_errors(errors))
}

const MANAGED_REGISTRY_PATHS: &[&str] = &[
    "state/registry/bindings.json",
    "state/registry/rules.json",
    "state/registry/targets.json",
    "state/registry/projections.json",
    "state/registry/ops/checkpoint.json",
    "state/registry/trust.json",
    "state/registry/sources.json",
    "loom.lock",
];

pub(super) fn uncommitted_source_head_is_preserved(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    head: &str,
    relative_source: &Path,
) -> std::result::Result<bool, CommandFailure> {
    let ancestor = gitops::run_git_allow_failure(
        &app.ctx,
        &["merge-base", "--is-ancestor", &journal.previous_head, head],
    )
    .map_err(map_git)?;
    if !ancestor.status.success() {
        return Ok(false);
    }
    let range = format!("{}..{head}", journal.previous_head);
    let source_path = relative_source.to_string_lossy();
    let commits = gitops::run_git(
        &app.ctx,
        &["rev-list", "--reverse", &range, "--", source_path.as_ref()],
    )
    .map_err(map_git)?;
    let commits = commits.lines().collect::<Vec<_>>();
    if commits.is_empty() {
        return Ok(true);
    }
    if commits.len() != 1
        || journal.phase != TransactionPhase::CommittingSource
        || journal.source_index_changed != Some(true)
    {
        return Ok(false);
    }
    let commit = commits[0];
    if verify_commit(
        app,
        commit,
        &journal.previous_head,
        &format!("skill({}): converge source", plan.skill),
        |path| path == source_path || path.starts_with(&format!("{source_path}/")),
    )
    .is_err()
    {
        return Ok(false);
    }
    Ok(
        super::recovery_evidence::committed_skill_digest(app, commit, &plan.skill)?
            == plan.input.selected_input_tree_digest,
    )
}

pub(super) fn handle_external_registry_failure(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
    failure: CommandFailure,
) -> std::result::Result<CommandFailure, CommandFailure> {
    let errors = retire_registry_after_external_head(app, paths, plan, journal_path, journal)?
        .unwrap_or_default();
    Ok(failure.with_rollback_errors(errors))
}

pub(super) fn recover_registry_after_external_head(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<bool, CommandFailure> {
    let Some(errors) =
        retire_registry_after_external_head(app, paths, plan, journal_path, journal)?
    else {
        return Ok(false);
    };
    if errors.is_empty() {
        return Ok(true);
    }
    Err(CommandFailure::new(
        ErrorCode::StateCorrupt,
        "external HEAD registry recovery failed",
    )
    .with_rollback_errors(errors))
}

fn retire_registry_after_external_head(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<Vec<Value>>, CommandFailure> {
    if let Some(commit) = journal.registry_commit.as_deref() {
        let head = gitops::head(&app.ctx).map_err(map_git)?;
        if head != commit
            && gitops::run_git_allow_failure(
                &app.ctx,
                &["merge-base", "--is-ancestor", commit, &head],
            )
            .map_err(map_git)?
            .status
            .success()
        {
            return Ok(None);
        }
    }
    if !external_head_preserves_reviewed_boundaries(app, plan, journal)? {
        return Ok(None);
    }
    super::registry_commit::discard_retained_registry_index_locks(app, journal)?;
    if !external_head_preserves_reviewed_boundaries(app, plan, journal)? {
        return Ok(None);
    }
    let mut errors = super::rollback::restore_registry_and_activated_projections(
        paths,
        plan,
        journal_path,
        journal,
    );
    if errors.is_empty() {
        super::source_commit::validate_live_source(app, plan)?;
        journal.registry_commit = None;
        journal.registry_staged_index_digest = None;
        journal.rollback_head = Some(gitops::head(&app.ctx).map_err(map_git)?);
        journal.rollback_index_digest = Some(active_index_digest(app)?);
        errors = cleanup_declared_artifacts(app, journal_path, journal, false);
    }
    if errors.is_empty() {
        archive_rolled_back_journal(journal_path, journal)?;
    }
    Ok(Some(errors))
}

pub(super) fn external_head_preserves_reviewed_boundaries(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
) -> std::result::Result<bool, CommandFailure> {
    let source_head = journal
        .source_head
        .as_deref()
        .ok_or_else(|| CommandFailure::new(ErrorCode::StateCorrupt, "source head is missing"))?;
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head == source_head {
        return Ok(false);
    }
    let ancestor = gitops::run_git_allow_failure(
        &app.ctx,
        &["merge-base", "--is-ancestor", source_head, &head],
    )
    .map_err(map_git)?;
    if !ancestor.status.success() {
        return Ok(false);
    }
    let committed = gitops::run_git(
        &app.ctx,
        &["show", &format!("{head}:state/registry/projections.json")],
    )
    .ok()
    .and_then(|raw| serde_json::from_str::<RegistryProjectionsFile>(&raw).ok());
    if committed
        .as_ref()
        .and_then(|value| serde_json::to_value(value).ok())
        != serde_json::to_value(&journal.original_projections).ok()
    {
        return Ok(false);
    }
    let range = format!("{source_head}..{head}");
    let skill_path = format!("skills/{}", plan.skill);
    let mut args = vec!["diff", "--quiet", range.as_str(), "--", skill_path.as_str()];
    args.extend(MANAGED_REGISTRY_PATHS.iter().copied());
    let unchanged = gitops::run_git_allow_failure(&app.ctx, &args).map_err(map_git)?;
    Ok(unchanged.status.success())
}

pub(super) fn validate_committed_managed_surfaces(
    app: &App,
    plan: &SkillConvergencePlan,
    boundary: &str,
) -> std::result::Result<(), CommandFailure> {
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head != boundary {
        let range = format!("{boundary}..{head}");
        let mut args = vec!["diff", "--quiet", range.as_str(), "--"];
        args.extend(MANAGED_REGISTRY_PATHS.iter().copied());
        if !gitops::run_git_allow_failure(&app.ctx, &args)
            .map_err(map_git)?
            .status
            .success()
        {
            return Err(recovery_stale(
                "descendant changed a committed registry or policy boundary",
            ));
        }
    }
    for path in MANAGED_REGISTRY_PATHS {
        for args in [
            vec!["diff", "--quiet", "--", path],
            vec!["diff", "--cached", "--quiet", "--", path],
        ] {
            if !gitops::run_git_allow_failure(&app.ctx, &args)
                .map_err(map_git)?
                .status
                .success()
            {
                return Err(recovery_stale(
                    "committed registry or policy surface has worktree drift",
                ));
            }
        }
    }
    super::guards::validate_recovery_routing(app, plan)
}

pub(super) fn complete_durable_registry_noop(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<Value>, CommandFailure> {
    if journal.phase != TransactionPhase::CommittingRegistry
        || !super::registry_commit::durable_registry_noop(journal)
    {
        return Ok(None);
    }
    let source_head = journal.source_head.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "no-op registry source head is missing",
        )
    })?;
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head != source_head && !external_head_preserves_reviewed_boundaries(app, plan, journal)? {
        return Err(recovery_stale(
            "descendant changed a reviewed no-op registry boundary",
        ));
    }
    super::recovery_evidence::reprove_source_boundary(app, plan, journal)?;
    validate_committed_managed_surfaces(app, plan, source_head)?;
    super::registry_recovery::validate_registry_result(app, plan, journal)?;
    let result = committed_result_with_registry(plan, journal, None);
    journal.result = Some(result.clone());
    journal.phase = TransactionPhase::CommittedCleanupPending;
    save_journal(journal_path, journal)?;
    finish_committed_cleanup(journal_path, journal)?;
    Ok(Some(result))
}

pub(super) fn retire_uncommitted_noop_after_external_head(
    app: &App,
    paths: &RegistryStatePaths,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<bool, CommandFailure> {
    if journal.phase != TransactionPhase::CommittingSource
        || journal.source_index_changed != Some(false)
        || journal.source_head.is_some()
        || journal.source_commit.is_some()
        || journal.installed_projections != 0
        || journal.expected_projections.is_some()
    {
        return Ok(false);
    }
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head == journal.previous_head {
        return Ok(false);
    }
    validate_mutated_surfaces(app, paths, plan, journal)?;
    validate_rollback_evidence(app, plan, journal)?;
    if plan.registry.initialized {
        let live = paths.load_projections().map_err(map_registry_state)?;
        if serde_json::to_value(live).map_err(map_io)?
            != serde_json::to_value(&journal.original_projections).map_err(map_io)?
        {
            return Err(recovery_stale(
                "registry changed during an uncommitted no-op source transaction",
            ));
        }
    } else if paths.exists() {
        return Err(recovery_stale(
            "registry initialized during an uncommitted no-op source transaction",
        ));
    }
    if plan.source.direction == ConvergenceInputDirection::Projection {
        restore_source_from_evidence(app, plan, journal)?;
    } else {
        super::source_commit::validate_live_source(app, plan)?;
    }
    journal.rollback_head = Some(head);
    journal.rollback_index_digest = Some(active_index_digest(app)?);
    let errors = cleanup_declared_artifacts(app, journal_path, journal, false);
    if !errors.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "uncommitted no-op source cleanup failed",
        )
        .with_rollback_errors(errors));
    }
    archive_rolled_back_journal(journal_path, journal)?;
    Ok(true)
}

pub(super) fn capture_current_rollback_evidence(
    app: &App,
    journal: &mut TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    journal.rollback_head = Some(gitops::head(&app.ctx).map_err(map_git)?);
    journal.rollback_index_digest = Some(active_index_digest(app)?);
    Ok(())
}
