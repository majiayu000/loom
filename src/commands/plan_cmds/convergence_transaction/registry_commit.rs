use super::recovery_evidence::file_digest;
use super::recovery_support::{validate_registry_result, verify_commit};
use super::*;

const REGISTRY_PATH: &str = "state/registry/projections.json";

pub(super) fn commit_convergence_registry(
    app: &App,
    plan: &SkillConvergencePlan,
    journal_path: &Path,
    journal: &mut TransactionJournal,
) -> std::result::Result<Option<String>, CommandFailure> {
    let source_head = journal.source_head.as_deref().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "journal is missing source head")
    })?;
    validate_registry_result(app, plan, journal)?;
    require_head(
        app,
        source_head,
        "registry commit parent changed before preparation",
    )?;

    let root = Path::new(&journal.artifact_root);
    let base_index = root.join("registry-base-index");
    let prepared_index = root.join("registry-index");
    let commit_index = root.join("registry-commit-index");
    reset_owned_files([&base_index, &prepared_index, &commit_index])?;
    gitops::snapshot_index_to(&app.ctx, &base_index).map_err(map_git)?;
    let base_index_digest = file_digest(&base_index)?;
    let changed =
        gitops::prepare_index_for_paths(&app.ctx, &base_index, &prepared_index, &[REGISTRY_PATH])
            .map_err(map_git)?;
    if !changed {
        require_head(app, source_head, "no-op registry commit changed HEAD")?;
        reset_owned_files([&base_index, &prepared_index, &commit_index])?;
        return Ok(None);
    }

    let message = format!("skill({}): record convergence projections", plan.skill);
    let commit = gitops::create_prepared_commit(
        &app.ctx,
        &prepared_index,
        &commit_index,
        &[REGISTRY_PATH],
        source_head,
        &message,
    )
    .map_err(map_git)?;
    verify_commit(app, &commit, source_head, &message, |path| {
        path == REGISTRY_PATH
    })?;
    let expected_index = file_digest(&prepared_index)?;
    journal.registry_commit = Some(commit.clone());
    journal.registry_staged_index_digest = Some(expected_index.clone());
    save_journal(journal_path, journal)?;
    let install =
        gitops::install_prepared_index_with_guard(&app.ctx, &prepared_index, |candidate| {
            validate_registry_result(app, plan, journal)
                .map_err(|error| anyhow::anyhow!(error.message))?;
            let actual = file_digest(candidate).map_err(|error| anyhow::anyhow!(error.message))?;
            if actual != expected_index {
                return Err(anyhow::anyhow!(
                    "prepared registry index changed after validation"
                ));
            }
            let active =
                active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
            if active != base_index_digest {
                return Err(anyhow::anyhow!(
                    "active Git index changed before registry index installation"
                ));
            }
            let head = gitops::head(&app.ctx)?;
            if head != source_head {
                return Err(anyhow::anyhow!(
                    "registry commit parent changed before compare-and-swap"
                ));
            }
            maybe_skill_fault("convergence_interrupt_before_registry_cas")
                .map_err(|error| anyhow::anyhow!(error.message))?;
            gitops::move_head_if_unchanged(&app.ctx, &commit, source_head)
        });
    if let Err(error) = install {
        if gitops::head(&app.ctx).map_err(map_git)? == commit {
            align_registry_index(app, plan, journal, &commit)?;
        } else {
            return Err(map_git(error));
        }
    }
    require_head(
        app,
        &commit,
        "registry commit compare-and-swap did not persist",
    )?;
    validate_registry_result(app, plan, journal)?;
    reset_owned_files([&base_index, &prepared_index, &commit_index])?;
    Ok(Some(commit))
}

pub(super) fn align_registry_index(
    app: &App,
    plan: &SkillConvergencePlan,
    journal: &TransactionJournal,
    expected_head: &str,
) -> std::result::Result<(), CommandFailure> {
    let staged = gitops::run_git_allow_failure(
        &app.ctx,
        &["diff", "--cached", "--quiet", "--", REGISTRY_PATH],
    )
    .map_err(map_git)?;
    if staged.status.success() {
        return Ok(());
    }
    validate_registry_result(app, plan, journal)?;
    if let Some(recorded) = journal.registry_commit.as_deref()
        && recorded != expected_head
    {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "recorded registry commit differs from HEAD",
        ));
    }
    let root = Path::new(&journal.artifact_root);
    let base_index = root.join("registry-repair-base-index");
    let prepared_index = root.join("registry-repair-index");
    reset_owned_files([&base_index, &prepared_index])?;
    gitops::snapshot_index_to(&app.ctx, &base_index).map_err(map_git)?;
    let base_index_digest = file_digest(&base_index)?;
    gitops::prepare_index_for_paths(&app.ctx, &base_index, &prepared_index, &[REGISTRY_PATH])
        .map_err(map_git)?;
    let expected_index = file_digest(&prepared_index)?;
    if let Some(recorded) = journal.registry_staged_index_digest.as_deref()
        && recorded != expected_index
    {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "registry recovery index differs from durable transaction evidence",
        ));
    }
    let recovered_lock =
        gitops::recover_prepared_index_lock_with_guard(&app.ctx, &prepared_index, |candidate| {
            let head = gitops::head(&app.ctx)?;
            if head != expected_head {
                return Err(anyhow::anyhow!(
                    "HEAD changed during registry lock recovery"
                ));
            }
            let actual = file_digest(candidate).map_err(|error| anyhow::anyhow!(error.message))?;
            if actual != expected_index {
                return Err(anyhow::anyhow!("recovered registry index lock changed"));
            }
            let active =
                active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
            if active != base_index_digest {
                return Err(anyhow::anyhow!(
                    "active Git index changed during registry lock recovery"
                ));
            }
            Ok(())
        })
        .map_err(map_git)?;
    if recovered_lock {
        return reset_owned_files([&base_index, &prepared_index]);
    }
    gitops::install_prepared_index_with_guard(&app.ctx, &prepared_index, |candidate| {
        let head = gitops::head(&app.ctx)?;
        if head != expected_head {
            return Err(anyhow::anyhow!("HEAD changed during registry index repair"));
        }
        let actual = file_digest(candidate).map_err(|error| anyhow::anyhow!(error.message))?;
        if actual != expected_index {
            return Err(anyhow::anyhow!("registry repair index changed"));
        }
        let active = active_index_digest(app).map_err(|error| anyhow::anyhow!(error.message))?;
        if active != base_index_digest {
            return Err(anyhow::anyhow!(
                "active Git index changed during registry index repair"
            ));
        }
        Ok(())
    })
    .map_err(map_git)?;
    reset_owned_files([&base_index, &prepared_index])
}

pub(super) fn require_head(
    app: &App,
    expected: &str,
    message: &str,
) -> std::result::Result<(), CommandFailure> {
    if gitops::head(&app.ctx).map_err(map_git)? == expected {
        Ok(())
    } else {
        Err(CommandFailure::new(ErrorCode::StateCorrupt, message))
    }
}

fn reset_owned_files<const N: usize>(paths: [&Path; N]) -> std::result::Result<(), CommandFailure> {
    for path in paths {
        remove_path_if_exists(path).map_err(map_io)?;
    }
    Ok(())
}
