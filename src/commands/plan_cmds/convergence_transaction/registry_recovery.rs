use super::recovery_support::{recovery_stale, validate_registry_result, verify_commit};
use super::*;

pub(super) fn prove_registry_boundary(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<String>, CommandFailure> {
    let source_head = journal.source_head.clone().ok_or_else(|| {
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
    if let Some(commit) = journal.registry_commit.clone() {
        super::recovery_evidence::verify_registry_commit(
            app,
            plan,
            journal,
            &commit,
            &source_head,
        )?;
        if head == commit {
            if let Some(commit) = super::registry_commit::resume_ready_registry_index_lock(
                app,
                plan,
                journal_path,
                journal,
                &source_head,
            )? {
                return Ok(Some(commit));
            }
            return Ok(Some(commit));
        }
        let committed_is_ancestor = gitops::run_git_allow_failure(
            &app.ctx,
            &["merge-base", "--is-ancestor", &commit, &head],
        )
        .map_err(map_git)?
        .status
        .success();
        if committed_is_ancestor {
            if !super::aggregate_audit::registry_commit_is_audit_only(journal) {
                return Err(recovery_stale(
                    "an intervening commit followed the registry boundary",
                ));
            }
            super::external_head::validate_committed_managed_surfaces_after_audit(
                app, plan, journal, &commit,
            )?;
            return Ok(Some(commit));
        }
        if head == source_head
            && let Some(commit) = super::registry_commit::resume_ready_registry_index_lock(
                app,
                plan,
                journal_path,
                journal,
                &source_head,
            )?
        {
            return Ok(Some(commit));
        }
        return Err(recovery_stale(
            "prepared registry commit is absent from the current HEAD ancestry",
        ));
    }
    if let Some(commit) = super::registry_commit::resume_ready_registry_index_lock(
        app,
        plan,
        journal_path,
        journal,
        &source_head,
    )? {
        return Ok(Some(commit));
    }
    if head == source_head {
        return super::registry_commit::commit_convergence_registry(
            app,
            plan,
            journal_path,
            journal,
        );
    }
    let candidates = gitops::run_git(
        &app.ctx,
        &["rev-list", "--reverse", &format!("{source_head}..{head}")],
    )
    .map_err(map_git)?;
    for candidate in candidates.lines() {
        if verify_commit(
            app,
            candidate,
            &source_head,
            &format!("skill({}): record convergence projections", plan.skill),
            super::aggregate_audit::is_registry_commit_path,
        )
        .is_ok()
        {
            super::external_head::validate_committed_managed_surfaces_after_audit(
                app, plan, journal, candidate,
            )?;
            return Ok(Some(candidate.to_string()));
        }
    }
    Err(recovery_stale(
        "registry convergence commit is absent from the current HEAD ancestry",
    ))
}
