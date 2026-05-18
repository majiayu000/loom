use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use serde_json::json;

use crate::cli::ProjectArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::remove_path_if_exists;
use crate::state_model::{
    RegistryBindingRule, RegistryProjectionInstance, RegistryStatePaths,
};

use super::super::fs_probe::probe_symlink;
use super::super::helpers::{
    backup_path_if_exists, commit_registry_state, ensure_skill_exists, map_arg, map_git, map_io,
    map_lock, map_project_io, map_registry_state, maybe_autosync_or_queue,
    project_skill_to_target, projection_instance_id, projection_method_as_str,
    record_registry_observation, record_registry_operation, restore_path_from_backup,
    restore_registry_audit_state, snapshot_registry_audit_state, upsert_projection, upsert_rule,
    validate_skill_name,
};
use super::super::{App, CommandFailure};
use super::shared::{maybe_skill_fault, rollback_registry_state};

impl App {
    pub fn cmd_project(
        &self,
        args: &ProjectArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;

        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let binding = snapshot.binding(&args.binding).cloned().ok_or_else(|| {
            CommandFailure::new(
                crate::types::ErrorCode::BindingNotFound,
                format!("binding '{}' not found", args.binding),
            )
        })?;

        let target_id = args
            .target
            .clone()
            .unwrap_or_else(|| binding.default_target_id.clone());
        let target = snapshot.target(&target_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                crate::types::ErrorCode::TargetNotFound,
                format!("target '{}' not found", target_id),
            )
        })?;

        if target.agent != binding.agent {
            return Err(CommandFailure::new(
                crate::types::ErrorCode::TargetAgentMismatch,
                format!(
                    "binding '{}' is for agent '{}' but target '{}' is for agent '{}'",
                    binding.binding_id, binding.agent, target.target_id, target.agent
                ),
            ));
        }

        if target.ownership != "managed" {
            return Err(CommandFailure::new(
                crate::types::ErrorCode::TargetNotManaged,
                format!(
                    "target '{}' has ownership '{}' and cannot be projected into",
                    target.target_id, target.ownership
                ),
            ));
        }

        super::super::helpers::validate_projection_method(&target, args.method)?;

        let skill_src = self.ctx.skill_path(&args.skill);
        let target_base = PathBuf::from(&target.path);
        fs::create_dir_all(&target_base).map_err(map_io)?;
        let materialized_path = target_base.join(&args.skill);

        if matches!(args.method, crate::cli::ProjectionMethod::Symlink) {
            let probe = probe_symlink(&target_base);
            if !probe.supported {
                return Err(CommandFailure::new(
                    crate::types::ErrorCode::ProjectionMethodUnsupported,
                    format!(
                        "target '{}' filesystem does not support symlink projection: {}. \
                         retry with --method copy",
                        target.target_id,
                        probe.reason.unwrap_or_else(|| "unknown reason".to_string())
                    ),
                ));
            }
        }

        let replaced_projection_backup =
            backup_path_if_exists(&self.ctx, &materialized_path, "project.replace_projection")
                .map_err(map_io)?;
        if let Err(err) = remove_path_if_exists(&materialized_path) {
            rollback_project_mutation(
                &paths,
                &materialized_path,
                replaced_projection_backup.as_ref(),
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            );
            return Err(map_io(err));
        }
        if let Err(err) = project_skill_to_target(&skill_src, &materialized_path, args.method) {
            rollback_project_mutation(
                &paths,
                &materialized_path,
                replaced_projection_backup.as_ref(),
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            );
            return Err(map_project_io(args.method)(err));
        }

        let original_bindings = snapshot.bindings.clone();
        let original_rules = snapshot.rules.clone();
        let original_projections = snapshot.projections.clone();
        let mut rules = original_rules.clone();
        upsert_rule(
            &mut rules,
            RegistryBindingRule {
                binding_id: binding.binding_id.clone(),
                skill_id: args.skill.clone(),
                target_id: target.target_id.clone(),
                method: projection_method_as_str(args.method).to_string(),
                watch_policy: "observe_only".to_string(),
                created_at: Some(Utc::now()),
            },
        );

        let mut projections = original_projections.clone();
        let instance_id =
            projection_instance_id(&args.skill, &binding.binding_id, &target.target_id);
        let projection = RegistryProjectionInstance {
            instance_id: instance_id.clone(),
            skill_id: args.skill.clone(),
            binding_id: Some(binding.binding_id.clone()),
            target_id: target.target_id.clone(),
            materialized_path: materialized_path.display().to_string(),
            method: projection_method_as_str(args.method).to_string(),
            last_applied_rev: gitops::head(&self.ctx).map_err(map_git)?,
            health: "healthy".to_string(),
            observed_drift: Some(false),
            updated_at: Some(Utc::now()),
        };
        upsert_projection(&mut projections, projection.clone());

        let registry_audit_backup =
            snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let post_materialize: std::result::Result<(Option<String>, Meta), CommandFailure> =
            (|| {
                maybe_skill_fault("skill_project_after_materialize")?;
                paths
                    .save_bindings_rules_projections(&original_bindings, &rules, &projections)
                    .map_err(map_registry_state)?;
                maybe_skill_fault("skill_project_after_state_save")?;
                let op_id = record_registry_operation(
                    &paths,
                    "skill.project",
                    json!({
                        "skill_id": args.skill,
                        "binding_id": binding.binding_id,
                        "target_id": target.target_id,
                        "method": projection_method_as_str(args.method),
                        "request_id": request_id
                    }),
                    json!({
                        "instance_id": instance_id
                    }),
                )
                .map_err(map_registry_state)?;
                record_registry_observation(
                    &paths,
                    &instance_id,
                    "projected",
                    Some(projection.materialized_path.clone()),
                    None,
                    Some(projection.last_applied_rev.clone()),
                )
                .map_err(map_registry_state)?;
                maybe_skill_fault("skill_project_after_observation")?;
                let commit = commit_registry_state(
                    &self.ctx,
                    &format!("project({}): record projection", args.skill),
                )?;
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                if let Some(commit) = &commit {
                    maybe_autosync_or_queue(
                        &self.ctx,
                        "skill.project",
                        request_id,
                        json!({
                            "skill": args.skill,
                            "binding_id": binding.binding_id,
                            "target_id": target.target_id,
                            "commit": commit
                        }),
                        &mut meta,
                    )?;
                }
                Ok((commit, meta))
            })();

        let (commit, meta) = match post_materialize {
            Ok(result) => result,
            Err(err) => {
                let _ = restore_registry_audit_state(&paths, &registry_audit_backup);
                rollback_project_mutation(
                    &paths,
                    &materialized_path,
                    replaced_projection_backup.as_ref(),
                    &original_bindings,
                    &original_rules,
                    &original_projections,
                );
                return Err(err);
            }
        };

        Ok((
            json!({"projection": projection, "backup": replaced_projection_backup, "commit": commit, "noop": false}),
            meta,
        ))
    }
}

fn rollback_project_mutation(
    paths: &RegistryStatePaths,
    materialized_path: &std::path::Path,
    backup: Option<&serde_json::Value>,
    original_bindings: &crate::state_model::RegistryBindingsFile,
    original_rules: &crate::state_model::RegistryRulesFile,
    original_projections: &crate::state_model::RegistryProjectionsFile,
) {
    if let Some(backup) = backup {
        let _ = restore_path_from_backup(materialized_path, backup);
    } else {
        let _ = crate::state::remove_path_if_exists(materialized_path);
    }
    rollback_registry_state(
        paths,
        original_bindings,
        original_rules,
        original_projections,
    );
}
