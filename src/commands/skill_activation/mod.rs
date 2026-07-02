mod apply;
mod plan;
mod resolve;

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::{ProjectionMethod, SkillActivateArgs, SkillActiveListArgs, SkillDeactivateArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::{RegistryBindingRule, RegistryProjectionInstance};
use crate::types::ErrorCode;

use super::helpers::{
    commit_registry_state, map_git, map_lock, map_registry_state, projection_instance_id,
    projection_method_as_str,
};
use super::projections::{
    maybe_autosync_or_queue, record_registry_observation, record_registry_operation,
    upsert_projection, upsert_rule,
};
use super::skill_compile::{CompiledActivationCandidate, compiled_activation_candidates};
use super::skill_safety::enforce_skill_safety;
use super::telemetry::{record_skill_activation_telemetry, telemetry_warning};
use super::{App, CommandFailure};

use apply::{
    apply_activation_projection, remove_safe_symlink_projection, restore_activation_state,
    save_activation_state,
};
use plan::{activation_plan, activation_state_changed, active_status, binding_matches_scope};
use resolve::{
    ActivationSelection, DEFAULT_POLICY_PROFILE, activation_selection,
    ensure_skill_exists_without_layout, optional_snapshot, resolve_activation,
    resolve_deactivation, scope_str, workspace_for_scope,
};

impl App {
    pub fn cmd_skill_activate(
        &self,
        args: &SkillActivateArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if args.artifact.is_some() && !args.compiled {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--artifact requires --compiled",
            ));
        }
        let selection = activation_selection(
            &args.skill,
            &args.agent,
            args.scope,
            args.workspace.clone(),
            args.profile.clone(),
            args.target.clone(),
            args.method,
        )?;
        ensure_skill_exists_without_layout(&self.ctx, &selection.skill)?;
        enforce_skill_safety(&self.ctx, &selection.skill, DEFAULT_POLICY_PROFILE)?;

        if args.compiled {
            let candidates = compiled_activation_candidates(
                &self.ctx,
                &selection.skill,
                args.artifact.as_deref(),
            )?;
            let reason = compiled_activation_block_reason(&selection, &candidates);
            return Err(compiled_activation_failure(
                &selection,
                args.artifact.as_deref(),
                reason,
                &candidates,
            ));
        }

        if args.dry_run {
            let snapshot = optional_snapshot(&self.ctx)?;
            let resolved = resolve_activation(&self.ctx, &snapshot, selection)?;
            return Ok((
                json!({"plan": activation_plan(&resolved, true), "dry_run": true}),
                Meta::default(),
            ));
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let resolved = resolve_activation(&self.ctx, &snapshot, selection)?;
        let plan = activation_plan(&resolved, false);

        let projection_changed = apply_activation_projection(&self.ctx, &resolved)?;
        let state_changed = activation_state_changed(&resolved) || projection_changed;
        if !state_changed {
            return Ok((json!({"plan": plan, "noop": true}), Meta::default()));
        }

        let original_targets = snapshot.targets.clone();
        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let mut targets = original_targets.clone();
        let mut bindings = original_bindings.clone();
        let mut rules = original_rules.clone();
        let mut projections = original_projections.clone();

        if resolved.target_is_new {
            targets.targets.push(resolved.target.clone());
            targets
                .targets
                .sort_by(|left, right| left.target_id.cmp(&right.target_id));
        }
        if resolved.binding_is_new {
            bindings.bindings.push(resolved.binding.clone());
            bindings
                .bindings
                .sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        }

        let rule = RegistryBindingRule {
            binding_id: resolved.binding.binding_id.clone(),
            skill_id: resolved.selection.skill.clone(),
            target_id: resolved.target.target_id.clone(),
            method: projection_method_as_str(resolved.selection.method).to_string(),
            watch_policy: "observe_only".to_string(),
            created_at: resolved
                .existing_rule
                .as_ref()
                .and_then(|rule| rule.created_at)
                .or_else(|| Some(Utc::now())),
        };
        upsert_rule(&mut rules, rule);

        let head = gitops::head(&self.ctx).map_err(map_git)?;
        let instance_id = projection_instance_id(
            &resolved.selection.skill,
            &resolved.binding.binding_id,
            &resolved.target.target_id,
        );
        let projection = RegistryProjectionInstance {
            instance_id: instance_id.clone(),
            skill_id: resolved.selection.skill.clone(),
            binding_id: Some(resolved.binding.binding_id.clone()),
            target_id: resolved.target.target_id.clone(),
            materialized_path: resolved.materialized_path.display().to_string(),
            method: projection_method_as_str(resolved.selection.method).to_string(),
            last_applied_rev: head.clone(),
            health: "healthy".to_string(),
            observed_drift: Some(false),
            updated_at: Some(Utc::now()),
        };
        upsert_projection(&mut projections, projection.clone());

        save_activation_state(
            &paths,
            &targets,
            &bindings,
            &rules,
            &projections,
            &original_targets,
        )?;
        let op_id = match record_registry_operation(
            &paths,
            "skill.activate",
            json!({
                "skill_id": resolved.selection.skill,
                "agent": resolved.selection.agent,
                "scope": scope_str(resolved.selection.scope),
                "profile": resolved.selection.profile,
                "binding_id": resolved.binding.binding_id,
                "target_id": resolved.target.target_id,
                "method": projection_method_as_str(resolved.selection.method),
                "request_id": request_id
            }),
            json!({"instance_id": instance_id}),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                restore_activation_state(
                    &paths,
                    &original_targets,
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                )?;
                return Err(map_registry_state(err));
            }
        };
        record_registry_observation(
            &paths,
            &instance_id,
            "activated",
            Some(projection.materialized_path.clone()),
            None,
            Some(head),
        )
        .map_err(map_registry_state)?;

        let commit = commit_registry_state(
            &self.ctx,
            &format!("activate({}): record skill activation", projection.skill_id),
        )?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "skill.activate",
                request_id,
                json!({
                    "skill": projection.skill_id,
                    "binding_id": projection.binding_id,
                    "target_id": projection.target_id,
                    "commit": commit
                }),
                &mut meta,
            )?;
        }
        if let Err(err) = record_skill_activation_telemetry(
            &self.ctx,
            &projection.skill_id,
            &resolved.selection.agent,
            true,
            resolved.selection.workspace.as_deref(),
        ) {
            meta.warnings
                .push(telemetry_warning("skill activation", &err));
        }

        Ok((
            json!({
                "plan": plan,
                "projection": projection,
                "target": resolved.target,
                "binding": resolved.binding,
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }

    pub fn cmd_skill_deactivate(
        &self,
        args: &SkillDeactivateArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let selection = activation_selection(
            &args.skill,
            &args.agent,
            args.scope,
            args.workspace.clone(),
            args.profile.clone(),
            args.target.clone(),
            ProjectionMethod::Symlink,
        )?;

        if args.dry_run {
            let snapshot = optional_snapshot(&self.ctx)?;
            let resolved = resolve_deactivation(&self.ctx, &snapshot, selection)?;
            return Ok((
                json!({"plan": plan::deactivation_plan(resolved.as_ref(), true), "dry_run": true}),
                Meta::default(),
            ));
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let Some(resolved) = resolve_deactivation(&self.ctx, &snapshot, selection)? else {
            return Ok((
                json!({"plan": plan::deactivation_plan(None, false), "noop": true}),
                Meta::default(),
            ));
        };
        let plan = plan::deactivation_plan(Some(&resolved), false);

        let rule = resolved.existing_rule.as_ref().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!(
                    "skill '{}' is not active for agent '{}'",
                    resolved.selection.skill, resolved.selection.agent
                ),
            )
        })?;
        if rule.method != "symlink" {
            return Err(CommandFailure::new(
                ErrorCode::PolicyBlocked,
                format!(
                    "deactivate refuses to delete '{}' projection '{}'; copy/materialize cleanup requires an explicit safe cleanup flow",
                    rule.method,
                    resolved.materialized_path.display()
                ),
            ));
        }

        remove_safe_symlink_projection(&self.ctx.skill_path(&resolved.selection.skill), &resolved)?;

        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let mut rules = original_rules.clone();
        let mut projections = original_projections.clone();
        rules.rules.retain(|item| {
            !(item.binding_id == resolved.binding.binding_id
                && item.skill_id == resolved.selection.skill
                && item.target_id == resolved.target.target_id)
        });
        let removed_instance_id = resolved
            .existing_projection
            .as_ref()
            .map(|projection| projection.instance_id.clone())
            .unwrap_or_else(|| {
                projection_instance_id(
                    &resolved.selection.skill,
                    &resolved.binding.binding_id,
                    &resolved.target.target_id,
                )
            });
        projections
            .projections
            .retain(|item| item.instance_id != removed_instance_id);
        paths
            .save_bindings_rules_projections(&original_bindings, &rules, &projections)
            .map_err(map_registry_state)?;

        let op_id = match record_registry_operation(
            &paths,
            "skill.deactivate",
            json!({
                "skill_id": resolved.selection.skill,
                "agent": resolved.selection.agent,
                "scope": scope_str(resolved.selection.scope),
                "profile": resolved.selection.profile,
                "binding_id": resolved.binding.binding_id,
                "target_id": resolved.target.target_id,
                "request_id": request_id
            }),
            json!({"instance_id": removed_instance_id}),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                paths
                    .save_bindings_rules_projections(
                        &original_bindings,
                        &original_rules,
                        &original_projections,
                    )
                    .map_err(map_registry_state)?;
                return Err(map_registry_state(err));
            }
        };
        record_registry_observation(
            &paths,
            &removed_instance_id,
            "deactivated",
            Some(resolved.materialized_path.display().to_string()),
            None,
            None,
        )
        .map_err(map_registry_state)?;

        let commit = commit_registry_state(
            &self.ctx,
            &format!(
                "deactivate({}): remove skill activation",
                resolved.selection.skill
            ),
        )?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "skill.deactivate",
                request_id,
                json!({
                    "skill": resolved.selection.skill,
                    "binding_id": resolved.binding.binding_id,
                    "target_id": resolved.target.target_id,
                    "commit": commit
                }),
                &mut meta,
            )?;
        }
        if let Err(err) = record_skill_activation_telemetry(
            &self.ctx,
            &resolved.selection.skill,
            &resolved.selection.agent,
            false,
            resolved.selection.workspace.as_deref(),
        ) {
            meta.warnings
                .push(telemetry_warning("skill deactivation", &err));
        }

        Ok((json!({"plan": plan, "commit": commit, "noop": false}), meta))
    }

    pub fn cmd_skill_active_list(
        &self,
        args: &SkillActiveListArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let agent = resolve::normalize_agent(&args.agent)?;
        let profile = args.profile.clone();
        let workspace = workspace_for_scope(args.scope, args.workspace.clone())?;
        let snapshot = optional_snapshot(&self.ctx)?;
        let mut items = Vec::new();

        for binding in &snapshot.bindings.bindings {
            if binding.agent != agent || !binding.active {
                continue;
            }
            if let Some(profile) = profile.as_deref()
                && binding.profile_id != profile
            {
                continue;
            }
            if !binding_matches_scope(binding, args.scope, workspace.as_deref()) {
                continue;
            }
            for rule in snapshot
                .rules
                .rules
                .iter()
                .filter(|rule| rule.binding_id == binding.binding_id)
            {
                let target = snapshot.target(&rule.target_id);
                let projection = snapshot.projections.projections.iter().find(|projection| {
                    projection.skill_id == rule.skill_id
                        && projection.binding_id.as_deref() == Some(&binding.binding_id)
                        && projection.target_id == rule.target_id
                });
                let source_exists = self.ctx.skill_path(&rule.skill_id).is_dir();
                items.push(json!({
                    "skill": rule.skill_id,
                    "agent": binding.agent,
                    "scope": scope_str(args.scope),
                    "profile": binding.profile_id,
                    "binding_id": binding.binding_id,
                    "target_id": rule.target_id,
                    "target_path": target.map(|target| target.path.clone()),
                    "method": rule.method,
                    "desired": true,
                    "projected": projection.is_some(),
                    "materialized_path": projection.map(|projection| projection.materialized_path.clone()),
                    "status": active_status(source_exists, target, projection),
                    "visible_to_agent": "not_checked",
                    "restart_required": "not_checked",
                }));
            }
        }

        Ok((
            json!({
                "state_model": "registry",
                "agent": agent,
                "scope": scope_str(args.scope),
                "count": items.len(),
                "items": items,
                "visibility_claim": "not_checked"
            }),
            Meta::default(),
        ))
    }
}

fn compiled_activation_block_reason(
    selection: &ActivationSelection,
    candidates: &[CompiledActivationCandidate],
) -> &'static str {
    if candidates.is_empty()
        || candidates
            .iter()
            .all(|candidate| candidate.status == "missing")
    {
        return "compiled_artifact_missing";
    }
    if candidates.iter().any(|candidate| {
        candidate.valid
            && candidate.status == "valid"
            && !candidate.source_stale
            && candidate.agent.as_deref() == Some(selection.agent.as_str())
            && candidate.profile.as_deref() == Some(selection.profile.as_str())
    }) {
        return "compiled_activation_deferred";
    }
    let all_candidates_have_identity = candidates
        .iter()
        .all(|candidate| candidate.agent.is_some() && candidate.profile.is_some());
    if all_candidates_have_identity
        && !candidates.iter().any(|candidate| {
            candidate.agent.as_deref() == Some(selection.agent.as_str())
                && candidate.profile.as_deref() == Some(selection.profile.as_str())
        })
    {
        return "compiled_artifact_agent_profile_mismatch";
    }
    "compiled_artifact_not_valid"
}

fn compiled_activation_failure(
    selection: &ActivationSelection,
    artifact: Option<&str>,
    reason: &'static str,
    candidates: &[CompiledActivationCandidate],
) -> CommandFailure {
    let message = match reason {
        "compiled_artifact_missing" => format!(
            "compiled activation requires a compiled artifact for skill '{}' agent '{}' profile '{}'",
            selection.skill, selection.agent, selection.profile
        ),
        "compiled_artifact_agent_profile_mismatch" => format!(
            "compiled activation artifact does not match agent '{}' profile '{}'",
            selection.agent, selection.profile
        ),
        "compiled_activation_deferred" => {
            "compiled activation projection is not implemented yet".to_string()
        }
        _ => format!(
            "compiled activation requires a valid compiled artifact for skill '{}' agent '{}' profile '{}'",
            selection.skill, selection.agent, selection.profile
        ),
    };
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    let reports = candidates
        .iter()
        .map(|candidate| candidate.report.clone())
        .collect::<Vec<_>>();
    let mut next_actions = vec![format!(
        "loom skill compile {} --agent {} --profile {}",
        selection.skill, selection.agent, selection.profile
    )];
    next_actions.push(match artifact {
        Some(artifact) => format!(
            "loom skill compile verify {} --artifact {}",
            selection.skill, artifact
        ),
        None => format!("loom skill compile verify {}", selection.skill),
    });
    if reason == "compiled_activation_deferred" {
        next_actions.push(format!(
            "loom skill activate {} --agent {}",
            selection.skill, selection.agent
        ));
    }
    failure.details = json!({
        "reason": reason,
        "skill": selection.skill,
        "agent": selection.agent,
        "profile": selection.profile,
        "artifact": artifact,
        "reports": reports,
        "next_actions": next_actions,
    });
    failure
}
