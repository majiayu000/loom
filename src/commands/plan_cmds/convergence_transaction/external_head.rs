use super::*;

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
    let errors = cleanup_declared_artifacts(journal_path, journal);
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
