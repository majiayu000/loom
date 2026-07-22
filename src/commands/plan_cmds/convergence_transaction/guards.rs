use super::super::converge::digest_value;
use super::*;
use crate::commands::agent_cmds::planning_helpers::workspace_matches;
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
    validate_policy_gate(plan)?;
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

pub(super) fn validate_recovery_routing_after_legacy_audit(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<(), CommandFailure> {
    validate_routing_paths_clean(
        app,
        &[
            "state/registry/bindings.json",
            "state/registry/rules.json",
            "state/registry/targets.json",
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
            "registry initialization changed during legacy recovery",
            "PLAN_CHECKPOINT_DRIFT",
        ));
    }
    if let Some(snapshot) = snapshot.as_ref() {
        validate_projection_routing(snapshot, plan)?;
    } else if !plan.projections.is_empty() {
        return Err(stale(
            "projection routing disappeared during legacy recovery",
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
    validate_sealed_scope(snapshot, plan)?;
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
        let method_supported = match effect.method.as_str() {
            "symlink" => target.capabilities.symlink,
            "copy" | "materialize" => target.capabilities.copy,
            _ => false,
        };
        if target.ownership != crate::core::vocab::Ownership::Managed
            || effect.ownership != "managed"
        {
            return Err(effect_failure(
                effect,
                "planned target is no longer safely managed",
                "PLAN_OWNERSHIP_DRIFT",
            ));
        }
        if !method_supported {
            return Err(effect_failure(
                effect,
                "planned projection method is no longer supported by the target",
                "PLAN_METHOD_DRIFT",
            ));
        }
        validate_target_filesystem_scope(target, effect)?;
        if !binding.active
            || !rule_matches
            || binding.agent.as_str() != effect.agent
            || binding.profile_id != effect.profile
            || target.agent.as_str() != effect.agent
            || Path::new(&target.path).join(&plan.skill) != Path::new(&effect.materialized_path)
        {
            return Err(effect_failure(
                effect,
                "projection routing changed after planning",
                "PLAN_PROJECTION_DRIFT",
            ));
        }
    }
    Ok(())
}

fn validate_sealed_scope(
    snapshot: &crate::state_model::RegistrySnapshot,
    plan: &SkillConvergencePlan,
) -> std::result::Result<(), CommandFailure> {
    let workspace = plan.selectors.workspace.as_deref().map(Path::new);
    let mut resolved = snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.skill_id == plan.skill)
        .filter_map(|rule| {
            let Some(binding) = snapshot.binding(&rule.binding_id) else {
                return Some((
                    rule.binding_id.clone(),
                    rule.target_id.clone(),
                    "missing_binding".to_string(),
                ));
            };
            if !binding.active
                || plan
                    .selectors
                    .agent
                    .as_deref()
                    .is_some_and(|agent| binding.agent.as_str() != agent)
                || plan
                    .selectors
                    .profile
                    .as_deref()
                    .is_some_and(|profile| binding.profile_id != profile)
                || workspace.is_some_and(|workspace| {
                    !workspace_matches(
                        binding.workspace_matcher.kind.as_str(),
                        &binding.workspace_matcher.value,
                        workspace,
                    )
                })
            {
                return None;
            }
            Some((
                rule.binding_id.clone(),
                rule.target_id.clone(),
                rule.method.as_str().to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let mut sealed = plan
        .projections
        .iter()
        .map(|effect| {
            (
                effect.binding_id.clone(),
                effect.target_id.clone(),
                effect.method.clone(),
            )
        })
        .collect::<Vec<_>>();
    resolved.sort();
    sealed.sort();
    if resolved != sealed {
        return Err(scope_failure(
            "current selector resolution does not match the sealed projection effects",
            "PLAN_SELECTOR_SCOPE_DRIFT",
        ));
    }

    let visibility_required = plan.required_axes.contains(&ConvergenceAxis::Visibility);
    let mut visibility = plan
        .visibility
        .iter()
        .map(|item| {
            (
                item.agent.clone(),
                item.binding_id.clone(),
                item.target_id.clone(),
                item.required,
            )
        })
        .collect::<Vec<_>>();
    let mut expected_visibility = plan
        .projections
        .iter()
        .map(|effect| {
            (
                effect.agent.clone(),
                effect.binding_id.clone(),
                effect.target_id.clone(),
                visibility_required,
            )
        })
        .collect::<Vec<_>>();
    visibility.sort();
    expected_visibility.sort();
    if visibility != expected_visibility
        || plan
            .visibility
            .iter()
            .any(|item| item.check != "post_apply_adapter_read")
        || (plan.remote == RemotePolicy::Push)
            != plan
                .required_axes
                .contains(&ConvergenceAxis::RegistryTransport)
        || (plan.accept_restart_required && !visibility_required)
    {
        return Err(scope_failure(
            "sealed visibility or remote scope is internally inconsistent",
            "PLAN_SELECTOR_SCOPE_DRIFT",
        ));
    }

    let selected_instance = match plan.source.direction {
        ConvergenceInputDirection::Source => None,
        ConvergenceInputDirection::Projection => plan.source.input_instance.as_deref(),
    };
    if plan.selectors.input_instance.as_deref() != selected_instance
        || plan.input.selected_projection_instance.as_deref() != selected_instance
        || selected_instance.is_some_and(|instance| {
            !plan
                .projections
                .iter()
                .any(|effect| effect.instance_id == instance)
        })
    {
        return Err(scope_failure(
            "sealed input selector does not match the reviewed projection effects",
            "PLAN_SELECTOR_SCOPE_DRIFT",
        ));
    }
    Ok(())
}

fn validate_target_filesystem_scope(
    target: &crate::state_model::RegistryProjectionTarget,
    effect: &crate::core::convergence::ProjectionEffectPlan,
) -> std::result::Result<(), CommandFailure> {
    let target_path = Path::new(&target.path);
    let metadata = match fs::symlink_metadata(target_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && effect.effect == "create" => {
            return validate_missing_target_filesystem_scope(target_path, effect);
        }
        Err(error) => {
            return Err(effect_failure(
                effect,
                format!("planned target filesystem root cannot be inspected safely: {error}"),
                "PLAN_FILESYSTEM_SCOPE_DRIFT",
            ));
        }
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(effect_failure(
            effect,
            "planned target filesystem root is not a concrete directory",
            "PLAN_FILESYSTEM_SCOPE_DRIFT",
        ));
    }
    let canonical = target_path.canonicalize().map_err(|error| {
        effect_failure(
            effect,
            format!("planned target filesystem root cannot be resolved safely: {error}"),
            "PLAN_FILESYSTEM_SCOPE_DRIFT",
        )
    })?;
    if canonical != target_path {
        return Err(effect_failure(
            effect,
            "planned target filesystem root was redirected after planning",
            "PLAN_FILESYSTEM_SCOPE_DRIFT",
        ));
    }
    Ok(())
}

fn validate_missing_target_filesystem_scope(
    target_path: &Path,
    effect: &crate::core::convergence::ProjectionEffectPlan,
) -> std::result::Result<(), CommandFailure> {
    if !target_path.is_absolute()
        || Path::new(&effect.materialized_path).parent() != Some(target_path)
    {
        return Err(effect_failure(
            effect,
            "missing managed target is outside the sealed absolute effect scope",
            "PLAN_FILESYSTEM_SCOPE_DRIFT",
        ));
    }
    for ancestor in target_path.ancestors().skip(1) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                let canonical = ancestor.canonicalize().map_err(|error| {
                    effect_failure(
                        effect,
                        format!("managed target ancestor cannot be resolved safely: {error}"),
                        "PLAN_FILESYSTEM_SCOPE_DRIFT",
                    )
                })?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() || canonical != ancestor
                {
                    return Err(effect_failure(
                        effect,
                        "missing managed target has a redirected filesystem ancestor",
                        "PLAN_FILESYSTEM_SCOPE_DRIFT",
                    ));
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(effect_failure(
                    effect,
                    format!("managed target ancestor cannot be inspected safely: {error}"),
                    "PLAN_FILESYSTEM_SCOPE_DRIFT",
                ));
            }
        }
    }
    Err(effect_failure(
        effect,
        "missing managed target has no safe existing filesystem ancestor",
        "PLAN_FILESYSTEM_SCOPE_DRIFT",
    ))
}

fn validate_policy_gate(plan: &SkillConvergencePlan) -> std::result::Result<(), CommandFailure> {
    let checks = &plan.preflight.checks;
    let policy_digest = checks.get("policy_safe_capture_digest");
    if !policy_digest.is_some_and(|digest| sealed_digest_is_valid(digest))
        || checks.get("policy_decision").map(String::as_str) != Some("allowed")
    {
        return Err(scope_failure(
            "sealed policy evidence is missing, malformed, or blocked",
            "PLAN_POLICY_DRIFT",
        ));
    }
    let sealed_approvals = checks.get("policy_required_approvals_digest");
    let observed_approvals = digest_value(&json!(plan.required_approvals))?;
    if !sealed_approvals.is_some_and(|digest| sealed_digest_is_valid(digest))
        || sealed_approvals != Some(&observed_approvals)
    {
        return Err(scope_failure(
            "sealed approval requirements do not match the reviewed plan",
            "PLAN_APPROVAL_DRIFT",
        ));
    }
    Ok(())
}

fn sealed_digest_is_valid(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn effect_failure(
    effect: &crate::core::convergence::ProjectionEffectPlan,
    message: impl Into<String>,
    code: &str,
) -> CommandFailure {
    let mut failure = scope_failure(message, code);
    failure.details["effect"] = json!({
        "instance_id": effect.instance_id,
        "binding_id": effect.binding_id,
        "target_id": effect.target_id,
        "method": effect.method,
        "materialized_path": effect.materialized_path,
    });
    failure
}

fn scope_failure(message: impl Into<String>, code: &str) -> CommandFailure {
    plan_failure(
        ErrorCode::DependencyConflict,
        message,
        code,
        false,
        vec!["create and review a fresh convergence plan".to_string()],
        None,
    )
}
