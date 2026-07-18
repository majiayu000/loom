use super::recovery_evidence::{active_index_digest, committed_skill_digest, file_digest};
use super::recovery_support::{recovery_stale, verify_commit};
use super::*;

pub(super) fn commit_convergence_source(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<String>, CommandFailure> {
    journal.phase = TransactionPhase::CommittingSource;
    save_journal(journal_path, journal)?;
    let relative = format!("skills/{}", plan.skill);
    let prepared_index = Path::new(&journal.artifact_root).join("source-index");
    let changed = gitops::prepare_index_for_paths_force(
        &app.ctx,
        Path::new(&journal.index_backup),
        &prepared_index,
        &[&relative],
    )
    .map_err(map_git)?;
    journal.source_staged_index_digest = Some(file_digest(&prepared_index)?);
    save_journal(journal_path, journal)?;
    maybe_skill_fault("convergence_interrupt_after_staged_index_prepared")?;

    let commit = if changed {
        let original = journal
            .index_backup_digest
            .clone()
            .ok_or_else(|| CommandFailure::new(ErrorCode::StateCorrupt, "index digest missing"))?;
        let staged = journal.source_staged_index_digest.clone().ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "staged index digest missing")
        })?;
        let message = format!("skill({}): converge source", plan.skill);
        let commit_index = Path::new(&journal.artifact_root).join("source-commit-index");
        let commit = gitops::create_prepared_commit(
            &app.ctx,
            &prepared_index,
            &commit_index,
            &[&relative],
            &journal.previous_head,
            &message,
        )
        .map_err(map_git)?;
        verify_commit(app, &commit, &journal.previous_head, &message, |path| {
            path == relative || path.starts_with(&format!("{relative}/"))
        })?;
        let committed = committed_skill_digest(app, &commit, &plan.skill)?;
        if committed != plan.input.selected_input_tree_digest {
            return Err(recovery_stale(
                "prepared source commit tree does not match the reviewed convergence input",
            ));
        }
        validate_live_source(app, plan)?;
        gitops::install_prepared_index_with_guard(&app.ctx, &prepared_index, &|candidate| {
            validate_live_source(app, plan)
                .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
            let installed =
                file_digest(candidate).map_err(|error| anyhow::anyhow!(error.message))?;
            if installed != staged {
                return Err(anyhow::anyhow!(
                    "prepared Git index changed after its digest was persisted"
                ));
            }
            let live = active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
            if live != original {
                return Err(anyhow::anyhow!(
                    "active Git index changed before prepared index installation"
                ));
            }
            Ok(())
        })
        .map_err(map_git)?;
        maybe_skill_fault("convergence_interrupt_after_source_add")?;
        maybe_skill_fault("convergence_interrupt_after_staged_index_install")?;
        if let Err(error) = validate_live_source(app, plan) {
            return Err(restore_index_after_failed_commit(app, journal, error));
        }
        if let Err(error) =
            gitops::move_head_if_unchanged(&app.ctx, &commit, &journal.previous_head)
                .map_err(map_git)
        {
            let observed = gitops::head(&app.ctx).map_err(map_git)?;
            if source_cas_failure_requires_index_restore(&observed, &journal.previous_head) {
                return Err(restore_index_after_failed_commit(app, journal, error));
            }
            return Err(error);
        }
        maybe_skill_fault("convergence_interrupt_after_source_cas")?;
        Some(commit)
    } else {
        validate_live_source(app, plan)?;
        None
    };

    let expected_source_head = commit.as_deref().unwrap_or(&journal.previous_head);
    journal.source_head = Some(expected_source_head.to_string());
    journal.source_commit = commit.clone();
    let source_head = gitops::head(&app.ctx).map_err(map_git)?;
    if source_head != expected_source_head {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "HEAD changed after the source compare-and-swap",
        ));
    }
    validate_live_source(app, plan)?;
    maybe_skill_fault("convergence_interrupt_committing_source")?;
    journal.phase = TransactionPhase::SourceCommitted;
    save_journal(journal_path, journal)?;
    maybe_skill_fault("convergence_interrupt_after_source_commit")?;
    maybe_skill_fault("convergence_after_source_commit")?;
    Ok(commit)
}

fn source_cas_failure_requires_index_restore(observed: &str, previous: &str) -> bool {
    observed == previous
}

pub(super) fn validate_live_source(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<(), CommandFailure> {
    let live = skill_tree_digest(&app.ctx.skill_path(&plan.skill)).map_err(map_io)?;
    if live == plan.input.selected_input_tree_digest {
        Ok(())
    } else {
        Err(recovery_stale(
            "source changed after the prepared index was reviewed",
        ))
    }
}

fn restore_index_after_failed_commit(
    app: &App,
    journal: &TransactionJournal,
    error: CommandFailure,
) -> CommandFailure {
    match gitops::restore_index_from_backup(&app.ctx, Path::new(&journal.index_backup)) {
        Ok(()) => error,
        Err(restore) => error.with_rollback_errors(vec![json!({
            "step": "restore_git_index_after_prepared_commit_failure",
            "message": restore.to_string(),
        })]),
    }
}

#[cfg(test)]
mod tests {
    use super::source_cas_failure_requires_index_restore;

    #[test]
    fn external_head_advance_never_restores_the_transaction_index() {
        assert!(source_cas_failure_requires_index_restore("old", "old"));
        assert!(!source_cas_failure_requires_index_restore(
            "external", "old"
        ));
    }
}
