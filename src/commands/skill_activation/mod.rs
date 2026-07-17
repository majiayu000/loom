mod apply;
mod compiled;
mod plan;
pub(crate) mod resolve;

use serde_json::{Value, json};

use crate::cli::{ProjectionMethod, SkillActivateArgs, SkillActiveListArgs, SkillDeactivateArgs};
use crate::envelope::Meta;
use crate::error_actions::contextual_skill_action;
use crate::next_action_trace::observe_next_actions;
use crate::types::ErrorCode;

use super::helpers::{
    commit_registry_state, map_lock, map_registry_state, projection_instance_id,
    projection_method_as_str,
};
use super::projections::{
    maybe_autosync_or_queue, record_registry_observation, record_registry_operation,
};
use super::skill_compile::compiled_activation_candidates;
use super::skill_safety::enforce_skill_safety;
use super::telemetry::{record_skill_activation_telemetry, telemetry_warning};
use super::{App, CommandFailure};

use apply::remove_safe_symlink_projection;
use plan::{activation_plan, active_status, binding_matches_scope};
use resolve::{
    DEFAULT_POLICY_PROFILE, activation_selection, ensure_skill_exists_without_layout,
    optional_snapshot, resolve_activation, resolve_deactivation, scope_str, workspace_for_scope,
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
            return self.cmd_skill_activate_compiled(args, selection, candidates, request_id);
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

        let execution = super::projection_executor::execute_projection(
            &self.ctx,
            &paths,
            &snapshot,
            super::projection_executor::ProjectionExecutionInput {
                skill: resolved.selection.skill.clone(),
                binding: resolved.binding.clone(),
                binding_is_new: resolved.binding_is_new,
                target: resolved.target.clone(),
                target_is_new: resolved.target_is_new,
                materialized_path: resolved.materialized_path.clone(),
                method: resolved.selection.method,
                operation_intent: "skill.activate",
                operation_payload: json!({
                    "skill_id": resolved.selection.skill,
                    "agent": resolved.selection.agent,
                    "scope": scope_str(resolved.selection.scope),
                    "profile": resolved.selection.profile,
                    "binding_id": resolved.binding.binding_id,
                    "target_id": resolved.target.target_id,
                    "method": projection_method_as_str(resolved.selection.method),
                    "request_id": request_id
                }),
                observation_kind: "activated",
                request_id: request_id.to_string(),
                commit_message: format!(
                    "activate({}): record skill activation",
                    resolved.selection.skill
                ),
                replace_existing: false,
                safe_existing_noop: true,
                after_materialize_fault: None,
                after_state_save_fault: None,
                after_observation_fault: None,
                activation_after_projection_fault: true,
            },
        )?;
        if execution.noop {
            return Ok((json!({"plan": plan, "noop": true}), Meta::default()));
        }

        let projection = execution.projection.ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "projection executor did not return a projection for skill.activate",
            )
        })?;
        let mut meta = execution.meta;
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
                "commit": execution.commit,
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
            if let Some(resolved) = resolved.as_ref() {
                ensure_symlink_deactivation_rule(resolved)?;
            }
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

        if resolved.existing_rule.is_none() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!(
                    "skill '{}' is not active for agent '{}'",
                    resolved.selection.skill, resolved.selection.agent
                ),
            ));
        }
        ensure_symlink_deactivation_rule(&resolved)?;

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

fn ensure_symlink_deactivation_rule(
    resolved: &resolve::ActivationResolved,
) -> std::result::Result<(), CommandFailure> {
    let Some(rule) = resolved.existing_rule.as_ref() else {
        return Ok(());
    };
    if rule.method == crate::core::vocab::ProjectionMethod::Symlink {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        format!(
            "deactivate refuses to delete '{}' projection '{}'; copy/materialize cleanup requires an explicit safe cleanup flow",
            rule.method,
            resolved.materialized_path.display()
        ),
    );
    failure.next_actions = observe_next_actions(
        "skill.deactivate.unsafe_path",
        vec![contextual_skill_action(
            &resolved.selection.skill,
            "inspect the skill projection before choosing a safe cleanup flow",
        )],
    );
    Err(failure)
}
