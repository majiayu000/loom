use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;
use serde_json::json;

use crate::agent_adapters::{SOURCE_BUILT_IN, decorate_target_for_output, load_agent_adapters};
use crate::cli::{TargetAddArgs, TargetCommand, TargetOwnership, TargetShowArgs};
use crate::envelope::Meta;
use crate::state_model::RegistryProjectionTarget;
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, map_io, map_lock, map_registry_state, target_capabilities,
    target_ownership_as_str, unique_target_id_for_agent,
};
use super::registry_txn::{RegistryTxnCommit, RegistryTxnState, RegistryWriteTxn};
use super::target_safety::{ensure_managed_target_root_is_unique, target_paths_equivalent};
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
                    let snapshot = self.require_registry_snapshot()?;
                    let adapters = load_agent_adapters(&self.ctx)?;
                    json!({
                        "state_model": "registry",
                        "count": snapshot.targets.targets.len(),
                        "targets": snapshot.targets.targets.iter().map(|target| decorate_target_for_output(target, &adapters)).collect::<Vec<_>>()
                    })
                },
                Meta::default(),
            )),
            TargetCommand::Show(args) => {
                let snapshot = self.require_registry_snapshot()?;
                let adapters = load_agent_adapters(&self.ctx)?;
                let target = snapshot.target(&args.target_id).ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::TargetNotFound,
                        format!("target '{}' not found", args.target_id),
                    )
                })?;
                let relations = snapshot.target_relations(&target.target_id);

                Ok((
                    json!({
                        "state_model": "registry",
                        "target": decorate_target_for_output(target, &adapters),
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
        self.cmd_target_add_raw(
            agent_kind_as_str(args.agent),
            &args.path,
            args.ownership,
            SOURCE_BUILT_IN,
            request_id,
        )
    }

    pub(crate) fn cmd_target_add_raw(
        &self,
        agent: &str,
        path: &str,
        ownership: TargetOwnership,
        agent_source: &str,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let target_path = PathBuf::from(path);
        if !target_path.is_absolute() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--path must be absolute",
            ));
        }

        match ownership {
            TargetOwnership::Managed => fs::create_dir_all(&target_path).map_err(map_io)?,
            TargetOwnership::Observed | TargetOwnership::External => {
                if !target_path.exists() {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        format!(
                            "target path '{}' must exist for ownership '{}'",
                            target_path.display(),
                            target_ownership_as_str(ownership)
                        ),
                    ));
                }
            }
        }
        let normalized_target_path = target_path
            .canonicalize()
            .with_context(|| {
                format!(
                    "failed to canonicalize target path '{}'",
                    target_path.display()
                )
            })
            .map_err(map_io)?;
        let normalized_path = normalized_target_path.to_string_lossy().into_owned();

        let paths = self.ensure_registry_layout()?;
        let mut targets = paths.load_targets().map_err(map_registry_state)?;
        let original_targets = targets.clone();

        if let Some(existing) = targets
            .targets
            .iter()
            .find(|target| {
                target.agent == agent
                    && target_paths_equivalent(&target.path, &normalized_target_path)
            })
            .cloned()
        {
            if existing.ownership != target_ownership_as_str(ownership) {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "target '{}' already exists with ownership '{}'",
                        existing.target_id, existing.ownership
                    ),
                ));
            }
            if ownership == TargetOwnership::Managed {
                ensure_managed_target_root_is_unique(
                    &targets.targets,
                    &normalized_target_path,
                    Some(&existing.target_id),
                )?;
            }
            let adapters = load_agent_adapters(&self.ctx)?;
            return Ok((
                json!({"target": decorate_target_for_output(&existing, &adapters), "noop": true}),
                Meta::default(),
            ));
        }

        if ownership == TargetOwnership::Managed {
            ensure_managed_target_root_is_unique(&targets.targets, &normalized_target_path, None)?;
        }

        let target_id = unique_target_id_for_agent(agent, &normalized_path, &targets);
        let target = RegistryProjectionTarget {
            target_id: target_id.clone(),
            agent: agent.into(),
            path: normalized_path,
            ownership,
            capabilities: target_capabilities(ownership),
            created_at: Some(Utc::now()),
        };

        targets.targets.push(target.clone());
        targets
            .targets
            .sort_by(|left, right| left.target_id.cmp(&right.target_id));
        let outcome = self.registry_write_txn(
            &paths,
            RegistryWriteTxn {
                op_name: "target.add",
                request_id,
                state: RegistryTxnState::Targets {
                    next: targets,
                    original: original_targets,
                },
                op_payload: json!({
                    "target_id": target.target_id,
                    "agent": target.agent,
                    "agent_source": agent_source,
                    "path": target.path,
                    "ownership": target.ownership,
                    "request_id": request_id
                }),
                op_effects: json!({
                    "target_id": target.target_id
                }),
                op_failure_note: None,
                commit: Some(RegistryTxnCommit {
                    message: format!("target({}): add", target_id),
                    autosync_payload: json!({"target_id": target.target_id}),
                }),
            },
        )?;
        let commit = outcome.commit;
        let meta = outcome.meta;
        let mut target_value = serde_json::to_value(&target).map_err(|err| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("failed to serialize target '{}': {err}", target.target_id),
            )
        })?;
        target_value["agent_source"] = json!(agent_source);

        Ok((
            json!({
                "target": target_value,
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }

    pub(crate) fn cmd_target_adopt_managed(
        &self,
        agent: &str,
        path: &Path,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        if !path.is_absolute() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "target path must be absolute",
            ));
        }
        fs::create_dir_all(path).map_err(map_io)?;
        let normalized_target_path = path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize target path '{}'", path.display()))
            .map_err(map_io)?;
        let normalized_path = normalized_target_path.to_string_lossy().into_owned();

        let paths = self.ensure_registry_layout()?;
        let mut targets = paths.load_targets().map_err(map_registry_state)?;
        let original_targets = targets.clone();
        let adapters = load_agent_adapters(&self.ctx)?;

        let Some(index) = targets.targets.iter().position(|target| {
            target.agent == agent && target_paths_equivalent(&target.path, &normalized_target_path)
        }) else {
            drop(_workspace);
            return self.cmd_target_add_raw(
                agent,
                &normalized_path,
                TargetOwnership::Managed,
                SOURCE_BUILT_IN,
                request_id,
            );
        };

        ensure_managed_target_root_is_unique(
            &targets.targets,
            &normalized_target_path,
            Some(&targets.targets[index].target_id),
        )?;

        if targets.targets[index].ownership == target_ownership_as_str(TargetOwnership::Managed) {
            let existing = targets.targets[index].clone();
            return Ok((
                json!({"target": decorate_target_for_output(&existing, &adapters), "noop": true}),
                Meta::default(),
            ));
        }

        let previous_ownership = targets.targets[index].ownership;
        targets.targets[index].ownership = crate::core::vocab::Ownership::Managed;
        targets.targets[index].capabilities = target_capabilities(TargetOwnership::Managed);
        let target = targets.targets[index].clone();
        let outcome = self.registry_write_txn(
            &paths,
            RegistryWriteTxn {
                op_name: "target.adopt",
                request_id,
                state: RegistryTxnState::Targets {
                    next: targets,
                    original: original_targets,
                },
                op_payload: json!({
                    "target_id": target.target_id,
                    "agent": target.agent,
                    "path": target.path,
                    "previous_ownership": previous_ownership,
                    "ownership": target.ownership,
                    "request_id": request_id
                }),
                op_effects: json!({
                    "target_id": target.target_id
                }),
                op_failure_note: None,
                commit: Some(RegistryTxnCommit {
                    message: format!("target({}): adopt", target.target_id),
                    autosync_payload: json!({"target_id": target.target_id}),
                }),
            },
        )?;
        let commit = outcome.commit;
        let meta = outcome.meta;
        Ok((
            json!({
                "target": decorate_target_for_output(&target, &adapters),
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }

    fn cmd_target_remove(
        &self,
        args: &TargetShowArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let original_targets = snapshot.targets.clone();
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
        let outcome = self.registry_write_txn(
            &paths,
            RegistryWriteTxn {
                op_name: "target.remove",
                request_id,
                state: RegistryTxnState::Targets {
                    next: snapshot.targets,
                    original: original_targets,
                },
                op_payload: json!({
                    "target_id": target.target_id,
                    "request_id": request_id
                }),
                op_effects: json!({
                    "target_id": target.target_id
                }),
                op_failure_note: None,
                commit: Some(RegistryTxnCommit {
                    message: format!("target({}): remove", args.target_id),
                    autosync_payload: json!({"target_id": target.target_id}),
                }),
            },
        )?;
        let commit = outcome.commit;
        let meta = outcome.meta;

        Ok((
            json!({
                "target": target,
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }
}
