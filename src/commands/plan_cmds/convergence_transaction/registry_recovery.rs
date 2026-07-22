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
        verify_commit(
            app,
            &commit,
            &source_head,
            &format!("skill({}): record convergence projections", plan.skill),
            |path| path == "state/registry/projections.json",
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
            super::external_head::validate_committed_managed_surfaces(app, plan, &commit)?;
            return Ok(Some(commit));
        }
        if head != source_head {
            return Err(recovery_stale(
                "HEAD no longer contains the transaction-created registry commit",
            ));
        }
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
    verify_commit(
        app,
        &head,
        &source_head,
        &format!("skill({}): record convergence projections", plan.skill),
        |path| path == "state/registry/projections.json",
    )?;
    super::registry_commit::align_registry_index(app, plan, journal_path, journal, &head)?;
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
    if snapshot.projections != *expected {
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
    local_axes: &Value,
) -> Value {
    let registry_operation = super::registry_operation_evidence();
    let evidence = json!({
        "source": { "direction": plan.source.direction, "commit": journal.source_commit },
        "projections": local_axes["projections"],
        "registry_operation": registry_operation,
        "visibility": local_axes["visibility"],
        "remote": {
            "state": if matches!(plan.remote, crate::core::convergence::RemotePolicy::NotRequested) {
                "not_requested"
            } else {
                "pending_push"
            },
        },
        "recovery": { "state": "journaled", "journal_phase": "committing_registry" },
    });
    json!({
        "skill": plan.skill,
        "source_commit": journal.source_commit,
        "registry_commit": registry_commit,
        "registry_operation": registry_operation,
        "projection_instances": plan.projections.iter().map(|item| item.instance_id.clone()).collect::<Vec<_>>(),
        "evidence": evidence,
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
