use super::recovery_evidence::{active_index_digest, file_digest};
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
    let changed = gitops::prepare_index_for_paths(
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
        gitops::install_prepared_index_with_guard(&app.ctx, &prepared_index, || {
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
        gitops::commit_prepared_paths(
            &app.ctx,
            &[&relative],
            &format!("skill({}): converge source", plan.skill),
        )
        .map_err(map_git)?
    } else {
        None
    };

    maybe_skill_fault("convergence_interrupt_committing_source")?;
    journal.source_head = Some(gitops::head(&app.ctx).map_err(map_git)?);
    journal.source_commit = commit.clone();
    journal.phase = TransactionPhase::SourceCommitted;
    save_journal(journal_path, journal)?;
    maybe_skill_fault("convergence_interrupt_after_source_commit")?;
    maybe_skill_fault("convergence_after_source_commit")?;
    Ok(commit)
}
