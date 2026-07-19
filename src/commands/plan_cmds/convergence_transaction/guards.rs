use super::super::converge::digest_value;
use super::*;
use crate::core::convergence::{ConvergenceAxis, RemotePolicy};

pub(super) fn validate_guards(
    app: &App,
    plan: &SkillConvergencePlan,
    cursor: usize,
) -> std::result::Result<Option<crate::state_model::RegistrySnapshot>, CommandFailure> {
    if !plan.input_conflicts.is_empty() || !plan.preflight.mutation_allowed {
        return Err(plan_failure(
            ErrorCode::PolicyBlocked,
            "convergence plan contains unresolved conflicts",
            "PLAN_NOT_SAFE_TO_APPLY",
            false,
            vec!["resolve conflicts and create a fresh plan".to_string()],
            Some(cursor),
        ));
    }
    if plan.remote != RemotePolicy::NotRequested
        || plan.required_axes.contains(&ConvergenceAxis::Visibility)
    {
        return Err(plan_failure(
            ErrorCode::PolicyBlocked,
            "requested post-local convergence axes are not executable in this tranche",
            "CONVERGENCE_POST_LOCAL_UNAVAILABLE",
            false,
            vec!["create a local-only convergence plan".to_string()],
            Some(cursor),
        ));
    }
    validate_pre_mutation_state(app, plan)
}

pub(super) fn validate_pre_mutation_recovery_guards(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<Option<crate::state_model::RegistrySnapshot>, CommandFailure> {
    validate_pre_mutation_state(app, plan)
}

fn validate_pre_mutation_state(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<Option<crate::state_model::RegistrySnapshot>, CommandFailure> {
    let head = gitops::head(&app.ctx).map_err(map_git)?;
    if head != plan.source.registry_head {
        return Err(stale("registry HEAD changed after planning", "PLAN_STALE"));
    }
    let source_digest = skill_tree_digest(&app.ctx.skill_path(&plan.skill)).map_err(map_io)?;
    if source_digest != plan.source.tree_digest {
        return Err(stale(
            "canonical source changed after planning",
            "PLAN_SOURCE_DRIFT",
        ));
    }
    validate_routing_paths_clean(
        app,
        &[
            "state/registry/bindings.json",
            "state/registry/rules.json",
            "state/registry/targets.json",
            "state/registry/projections.json",
            "state/registry/ops/checkpoint.json",
            "state/registry/trust.json",
            "state/registry/sources.json",
            "loom.lock",
        ],
    )?;
    let paths = RegistryStatePaths::from_app_context(&app.ctx);
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    if plan.registry.initialized != snapshot.is_some() {
        return Err(stale(
            "registry initialization changed after planning",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    if let Some(snapshot) = snapshot.as_ref() {
        validate_checkpoint_evidence(snapshot, plan)?;
        let projections_digest = digest_value(&snapshot.projections)?;
        if plan.registry.projections_digest.as_deref() != Some(projections_digest.as_str()) {
            return Err(stale(
                "registry checkpoint changed after planning",
                "PLAN_CHECKPOINT_DRIFT",
            ));
        }
        validate_projection_routing(snapshot, plan)?;
        for effect in &plan.projections {
            validate_projection_guard(app, plan, effect)?;
        }
    } else if !plan.projections.is_empty() {
        return Err(stale(
            "projection routing disappeared after planning",
            "PLAN_PROJECTION_DRIFT",
        ));
    }
    Ok(snapshot)
}

pub(super) fn validate_recovery_routing(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<(), CommandFailure> {
    validate_routing_paths_clean(
        app,
        &[
            "state/registry/bindings.json",
            "state/registry/rules.json",
            "state/registry/targets.json",
            "state/registry/ops/checkpoint.json",
            "state/registry/trust.json",
            "state/registry/sources.json",
            "loom.lock",
        ],
    )?;
    let snapshot = RegistryStatePaths::from_app_context(&app.ctx)
        .maybe_load_snapshot()
        .map_err(map_registry_state)?;
    if plan.registry.initialized != snapshot.is_some() {
        return Err(stale(
            "registry initialization changed during recovery",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    if let Some(snapshot) = snapshot.as_ref() {
        validate_checkpoint_evidence(snapshot, plan)?;
        validate_projection_routing(snapshot, plan)?;
    } else if !plan.projections.is_empty() {
        return Err(stale(
            "projection routing disappeared during recovery",
            "PLAN_PROJECTION_DRIFT",
        ));
    }
    Ok(())
}

fn validate_checkpoint_evidence(
    snapshot: &crate::state_model::RegistrySnapshot,
    plan: &SkillConvergencePlan,
) -> std::result::Result<(), CommandFailure> {
    let digest = digest_value(&snapshot.checkpoint)?;
    if plan.registry.checkpoint_digest.as_deref() != Some(digest.as_str())
        || plan.registry.checkpoint_updated_at.as_deref()
            != Some(snapshot.checkpoint.updated_at.to_rfc3339().as_str())
    {
        return Err(stale(
            "registry checkpoint changed after planning",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    Ok(())
}

fn validate_routing_paths_clean(
    app: &App,
    paths: &[&str],
) -> std::result::Result<(), CommandFailure> {
    for path in paths {
        if app.ctx.root.join(path).exists() {
            let committed = gitops::run_git_allow_failure(
                &app.ctx,
                &["cat-file", "-e", &format!("HEAD:{path}")],
            )
            .map_err(map_git)?;
            if !committed.status.success() {
                return Err(stale(
                    "registry routing exists without committed HEAD evidence",
                    "PLAN_CHECKPOINT_DRIFT",
                ));
            }
        }
        for args in [
            vec!["diff", "--quiet", "--", path],
            vec!["diff", "--cached", "--quiet", "--", path],
        ] {
            let output = gitops::run_git_allow_failure(&app.ctx, &args).map_err(map_git)?;
            if !output.status.success() {
                return Err(stale(
                    "registry routing changed after planning or is not committed",
                    "PLAN_CHECKPOINT_DRIFT",
                ));
            }
        }
    }
    Ok(())
}

fn validate_projection_routing(
    snapshot: &crate::state_model::RegistrySnapshot,
    plan: &SkillConvergencePlan,
) -> std::result::Result<(), CommandFailure> {
    for effect in &plan.projections {
        let binding = snapshot
            .binding(&effect.binding_id)
            .ok_or_else(|| stale("planned binding no longer exists", "PLAN_BINDING_DRIFT"))?;
        let target = snapshot
            .target(&effect.target_id)
            .ok_or_else(|| stale("planned target no longer exists", "PLAN_TARGET_DRIFT"))?;
        let rule_matches = snapshot.rules.rules.iter().any(|rule| {
            rule.binding_id == effect.binding_id
                && rule.skill_id == plan.skill
                && rule.target_id == effect.target_id
                && rule.method.as_str() == effect.method
        });
        if !binding.active
            || !rule_matches
            || binding.agent.as_str() != effect.agent
            || binding.profile_id != effect.profile
            || target.agent.as_str() != effect.agent
            || target.ownership.as_str() != effect.ownership
            || Path::new(&target.path).join(&plan.skill) != Path::new(&effect.materialized_path)
        {
            return Err(stale(
                "projection routing changed after planning",
                "PLAN_PROJECTION_DRIFT",
            ));
        }
    }
    Ok(())
}
