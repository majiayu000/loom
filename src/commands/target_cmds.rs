use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::json;

use crate::cli::{TargetAddArgs, TargetCommand, TargetOwnership, TargetShowArgs};
use crate::envelope::Meta;
use crate::state_model::V3ProjectionTarget;
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, map_io, map_lock, map_v3_state, record_v3_operation, target_capabilities,
    target_ownership_as_str, unique_target_id,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_target(
        &self,
        command: &TargetCommand,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            TargetCommand::Add(args) => self.cmd_target_add(args, request_id),
            TargetCommand::List => Ok((
                {
                    let snapshot = self.require_v3_snapshot()?;
                    json!({
                        "state_model": "v3",
                        "count": snapshot.targets.targets.len(),
                        "targets": snapshot.targets.targets
                    })
                },
                Meta::default(),
            )),
            TargetCommand::Show(args) => {
                let snapshot = self.require_v3_snapshot()?;
                let target = snapshot.target(&args.target_id).ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::TargetNotFound,
                        format!("target '{}' not found", args.target_id),
                    )
                })?;
                let relations = snapshot.target_relations(&target.target_id);

                Ok((
                    json!({
                        "state_model": "v3",
                        "target": target,
                        "bindings": relations.bindings,
                        "rules": relations.rules,
                        "projections": relations.projections
                    }),
                    Meta::default(),
                ))
            }
            TargetCommand::Remove(args) => self.cmd_target_remove(args, request_id),
        }
    }

    fn cmd_target_add(
        &self,
        args: &TargetAddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let target_path = PathBuf::from(&args.path);
        if !target_path.is_absolute() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--path must be absolute",
            ));
        }

        match args.ownership {
            TargetOwnership::Managed => fs::create_dir_all(&target_path).map_err(map_io)?,
            TargetOwnership::Observed | TargetOwnership::External => {
                if !target_path.exists() {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        format!(
                            "target path '{}' must exist for ownership '{}'",
                            target_path.display(),
                            target_ownership_as_str(args.ownership)
                        ),
                    ));
                }
            }
        }

        let paths = self.ensure_v3_layout()?;
        let mut targets = paths.load_targets().map_err(map_v3_state)?;

        if let Some(existing) = targets
            .targets
            .iter()
            .find(|target| {
                target.agent == agent_kind_as_str(args.agent) && target.path == args.path
            })
            .cloned()
        {
            if existing.ownership != target_ownership_as_str(args.ownership) {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "target '{}' already exists with ownership '{}'",
                        existing.target_id, existing.ownership
                    ),
                ));
            }
            return Ok((json!({"target": existing, "noop": true}), Meta::default()));
        }

        let target_id = unique_target_id(&targets, args);
        let target = V3ProjectionTarget {
            target_id: target_id.clone(),
            agent: agent_kind_as_str(args.agent).to_string(),
            path: args.path.clone(),
            ownership: target_ownership_as_str(args.ownership).to_string(),
            capabilities: target_capabilities(args.ownership),
            created_at: Some(Utc::now()),
        };

        targets.targets.push(target.clone());
        targets
            .targets
            .sort_by(|left, right| left.target_id.cmp(&right.target_id));
        paths.save_targets(&targets).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "target.add",
            json!({
                "target_id": target.target_id,
                "agent": target.agent,
                "path": target.path,
                "ownership": target.ownership,
                "request_id": request_id
            }),
            json!({
                "target_id": target.target_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({"target": target, "noop": false}),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    fn cmd_target_remove(
        &self,
        args: &TargetShowArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let paths = self.ensure_v3_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let target = snapshot.target(&args.target_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", args.target_id),
            )
        })?;

        let relations = snapshot.target_relations(&args.target_id);
        let active_projections: Vec<_> = relations
            .projections
            .iter()
            .filter(|p| p.health != "orphaned")
            .collect();
        if !relations.bindings.is_empty()
            || !relations.rules.is_empty()
            || !active_projections.is_empty()
        {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                format!(
                    "target '{}' is still referenced; remove dependent bindings or projections first",
                    args.target_id
                ),
            );
            failure.details = json!({
                "binding_ids": relations.bindings.iter().map(|binding| binding.binding_id.clone()).collect::<Vec<_>>(),
                "rule_skills": relations.rules.iter().map(|rule| rule.skill_id.clone()).collect::<Vec<_>>(),
                "projection_ids": active_projections.iter().map(|p| p.instance_id.clone()).collect::<Vec<_>>(),
            });
            return Err(failure);
        }

        snapshot
            .targets
            .targets
            .retain(|item| item.target_id != args.target_id);
        paths
            .save_targets(&snapshot.targets)
            .map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "target.remove",
            json!({
                "target_id": target.target_id,
                "request_id": request_id
            }),
            json!({
                "target_id": target.target_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({
                "target": target,
                "noop": false
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }
}
