use std::path::Path;

use anyhow::Context;
use chrono::Utc;
use serde_json::json;

use crate::cli::{BindingAddArgs, BindingRemoveArgs, WorkspaceBindingCommand};
use crate::envelope::Meta;
use crate::state_model::{RegistryWorkspaceBinding, RegistryWorkspaceMatcher};
use crate::types::ErrorCode;

use super::super::helpers::{
    agent_kind_as_str, commit_registry_state, map_lock, map_registry_state, unique_binding_id,
    validate_non_empty, validate_policy_profile, workspace_matcher_kind_as_str,
};
use super::super::projections::{maybe_autosync_or_queue, record_registry_operation};
use super::super::{App, CommandFailure};

impl App {
    pub fn cmd_workspace_binding(
        &self,
        command: &WorkspaceBindingCommand,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            WorkspaceBindingCommand::Add(args) => self.cmd_workspace_binding_add(args, request_id),
            WorkspaceBindingCommand::List => Ok((
                {
                    let snapshot = self.require_registry_snapshot()?;
                    json!({
                        "state_model": "registry",
                        "count": snapshot.bindings.bindings.len(),
                        "bindings": snapshot.bindings.bindings
                    })
                },
                Meta::default(),
            )),
            WorkspaceBindingCommand::Show(args) => {
                let snapshot = self.require_registry_snapshot()?;
                let binding = snapshot.binding(&args.binding_id).cloned().ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::BindingNotFound,
                        format!("binding '{}' not found", args.binding_id),
                    )
                })?;
                let default_target = snapshot.binding_default_target(&binding);
                let rules = snapshot.binding_rules(&binding.binding_id);
                let projections = snapshot.binding_projections(&binding.binding_id);

                Ok((
                    json!({
                        "state_model": "registry",
                        "binding": binding,
                        "default_target": default_target,
                        "rules": rules,
                        "projections": projections
                    }),
                    Meta::default(),
                ))
            }
            WorkspaceBindingCommand::Remove(args) => {
                self.cmd_workspace_binding_remove(args, request_id)
            }
        }
    }

    fn cmd_workspace_binding_add(
        &self,
        args: &BindingAddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        validate_non_empty("profile", &args.profile)?;
        validate_non_empty("matcher_value", &args.matcher_value)?;
        validate_non_empty("target", &args.target)?;
        validate_policy_profile(&args.policy_profile)?;

        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let original_bindings = snapshot.bindings.clone();
        if snapshot.target(&args.target).is_none() {
            return Err(CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", args.target),
            ));
        }

        if let Some(existing) = snapshot
            .bindings
            .bindings
            .iter()
            .find(|binding| {
                binding.agent == agent_kind_as_str(args.agent)
                    && binding.profile_id == args.profile
                    && binding.workspace_matcher.kind
                        == workspace_matcher_kind_as_str(args.matcher_kind)
                    && binding.workspace_matcher.value == args.matcher_value
                    && binding.default_target_id == args.target
                    && binding.policy_profile == args.policy_profile
            })
            .cloned()
        {
            return Ok((json!({"binding": existing, "noop": true}), Meta::default()));
        }

        let mut bindings = snapshot.bindings;
        let binding_id = unique_binding_id(&bindings, args);
        let binding = RegistryWorkspaceBinding {
            binding_id: binding_id.clone(),
            agent: agent_kind_as_str(args.agent).into(),
            profile_id: args.profile.clone(),
            workspace_matcher: RegistryWorkspaceMatcher {
                kind: args.matcher_kind,
                value: args.matcher_value.clone(),
            },
            default_target_id: args.target.clone(),
            policy_profile: args.policy_profile.clone(),
            active: true,
            created_at: Some(Utc::now()),
        };

        bindings.bindings.push(binding.clone());
        bindings
            .bindings
            .sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        paths.save_bindings(&bindings).map_err(map_registry_state)?;

        let op_id = match record_registry_operation(
            &paths,
            "workspace.binding.add",
            json!({
                "binding_id": binding.binding_id,
                "agent": binding.agent,
                "profile_id": binding.profile_id,
                "matcher_kind": binding.workspace_matcher.kind,
                "matcher_value": binding.workspace_matcher.value,
                "target_id": binding.default_target_id,
                "request_id": request_id
            }),
            json!({
                "binding_id": binding.binding_id
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                paths
                    .save_bindings(&original_bindings)
                    .with_context(|| {
                        format!(
                            "failed to rollback bindings after operation-log failure: {}",
                            err
                        )
                    })
                    .map_err(map_registry_state)?;
                return Err(map_registry_state(err));
            }
        };
        let commit = commit_registry_state(&self.ctx, &format!("binding({}): add", binding_id))?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "workspace.binding.add",
                request_id,
                json!({"binding_id": binding.binding_id, "commit": commit}),
                &mut meta,
            )?;
        }

        Ok((
            json!({"binding": binding, "commit": commit, "noop": false}),
            meta,
        ))
    }

    pub(super) fn cmd_workspace_binding_remove(
        &self,
        args: &BindingRemoveArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let binding = snapshot.binding(&args.binding_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{}' not found", args.binding_id),
            )
        })?;

        let removed_rules = snapshot.binding_rules(&args.binding_id);
        let removed_projections = snapshot.binding_projections(&args.binding_id);
        let active_projections: Vec<_> = removed_projections
            .iter()
            .filter(|projection| projection.health != "orphaned")
            .collect();
        if !args.orphan_projections && !active_projections.is_empty() {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                format!(
                    "binding '{}' is still referenced; remove dependent projections first or rerun with --orphan-projections",
                    args.binding_id
                ),
            );
            failure.details = json!({
                "binding_id": args.binding_id,
                "projection_ids": active_projections
                    .iter()
                    .map(|projection| projection.instance_id.clone())
                    .collect::<Vec<_>>(),
                "orphan_flag": "--orphan-projections",
            });
            return Err(failure);
        }
        let orphaned_paths = removed_projections
            .iter()
            .map(|projection| projection.materialized_path.clone())
            .filter(|path| Path::new(path).exists())
            .collect::<Vec<_>>();

        snapshot
            .bindings
            .bindings
            .retain(|item| item.binding_id != args.binding_id);
        snapshot
            .rules
            .rules
            .retain(|item| item.binding_id != args.binding_id);
        let mut orphaned_projection_ids = Vec::new();
        for proj in snapshot.projections.projections.iter_mut() {
            if proj.binding_id.as_deref() == Some(&args.binding_id) {
                proj.binding_id = None;
                proj.health = crate::core::vocab::Health::Orphaned;
                orphaned_projection_ids.push(proj.instance_id.clone());
            }
        }

        paths
            .save_bindings_rules_projections(
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            )
            .map_err(map_registry_state)?;

        let op_id = match record_registry_operation(
            &paths,
            "workspace.binding.remove",
            json!({
                "binding_id": binding.binding_id,
                "request_id": request_id
            }),
            json!({
                "binding_id": binding.binding_id,
                "removed_rules": removed_rules.iter().map(|rule| rule.skill_id.clone()).collect::<Vec<_>>(),
                "orphaned_projection_ids": orphaned_projection_ids,
                "orphan_projections": args.orphan_projections,
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                paths
                    .save_bindings_rules_projections(
                        &original_bindings,
                        &original_rules,
                        &original_projections,
                    )
                    .with_context(|| {
                        format!(
                            "failed to rollback bindings after operation-log failure: {}",
                            err
                        )
                    })
                    .map_err(map_registry_state)?;
                return Err(map_registry_state(err));
            }
        };

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        let commit =
            commit_registry_state(&self.ctx, &format!("binding({}): remove", args.binding_id))?;
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "workspace.binding.remove",
                request_id,
                json!({"binding_id": binding.binding_id, "commit": commit}),
                &mut meta,
            )?;
        }
        if !orphaned_projection_ids.is_empty() {
            meta.warnings.push(format!(
                "binding removed; {} projection(s) marked orphaned - run `loom skill orphan clean` to remove metadata",
                orphaned_projection_ids.len()
            ));
        }

        Ok((
            json!({
                "binding": binding,
                "removed_rules": removed_rules,
                "orphaned_projections": removed_projections,
                "orphaned_projection_ids": orphaned_projection_ids,
                "orphaned_paths": orphaned_paths,
                "orphaned_count": orphaned_projection_ids.len(),
                "orphan_projections": args.orphan_projections,
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }
}
