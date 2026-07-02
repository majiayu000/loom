use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::{AgentKind, CodexReconcileArgs, ProjectionMethod, SkillVisibilityArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::RegistryProjectionInstance;
use crate::types::ErrorCode;

use super::codex_config::{
    CodexConfigLoad, load_codex_config, malformed_config_failure, patch_disabled_entries,
};
use super::codex_reconcile_plan::plan_codex_reconcile;
use super::codex_visibility::{
    CODEX_AGENT, CodexReconcileAction, CodexReconcileRequest, build_codex_visibility_report,
    path_exists_or_symlink, projection_path_is_safe_symlink,
};
use super::helpers::{
    commit_registry_state, map_git, map_io, map_lock, map_project_io, map_registry_state,
    projection_instance_id,
};
use super::projections::{
    maybe_autosync_or_queue, project_skill_to_target, record_registry_operation, upsert_projection,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_skill_visibility(
        &self,
        args: &SkillVisibilityArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_codex_agent(args.agent)?;
        let report = build_codex_visibility_report(
            &self.ctx,
            &args.skill,
            args.workspace.as_deref(),
            args.profile.as_deref(),
        )?;
        Ok((json!(report), Meta::default()))
    }

    pub fn cmd_codex_reconcile(
        &self,
        args: &CodexReconcileArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if args.apply && args.dry_run {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--apply and --dry-run cannot be used together",
            ));
        }
        if args.fix_config && !args.apply {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--fix-config requires --apply",
            ));
        }
        if args.fix_config
            && let CodexConfigLoad::Malformed(error) = load_codex_config()?
        {
            return Err(malformed_config_failure(&error));
        }

        if !args.apply {
            let snapshot = self.require_registry_snapshot()?;
            let request = reconcile_request(args, true);
            let plans = plan_codex_reconcile(&self.ctx, &snapshot, &request)?;
            return Ok((json!({"dry_run": true, "plans": plans}), Meta::default()));
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let request = reconcile_request(args, false);
        let plans = plan_codex_reconcile(&self.ctx, &snapshot, &request)?;
        let unsafe_actions = plans
            .iter()
            .flat_map(|plan| plan.actions.iter())
            .filter(|action| !action.safe)
            .map(|action| json!(action))
            .collect::<Vec<_>>();
        if !unsafe_actions.is_empty() {
            let mut failure = CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "Codex reconcile plan contains unsafe actions requiring manual review",
            );
            failure.details = json!({"unsafe_actions": unsafe_actions});
            return Err(failure);
        }

        let original_projections = snapshot.projections.clone();
        let mut projections = original_projections.clone();
        let mut applied_actions = Vec::new();
        let mut projection_state_changed = false;
        let head = gitops::head(&self.ctx).map_err(map_git)?;

        for action in plans.iter().flat_map(|plan| plan.actions.iter()) {
            match action.category.as_str() {
                "create_projection" | "repair_projection" => {
                    apply_projection_repair(&self.ctx, action, &mut projections, &head)?;
                    projection_state_changed = true;
                    applied_actions.push(json!(action));
                }
                "remove_stale_projection" => {
                    remove_stale_projection(action)?;
                    applied_actions.push(json!(action));
                }
                "remove_stale_record" => {
                    if remove_stale_record(action, &mut projections) {
                        projection_state_changed = true;
                    }
                    applied_actions.push(json!(action));
                }
                "fix_config_disable" if args.fix_config => {
                    applied_actions.push(json!(action));
                }
                _ => {}
            }
        }

        if projection_state_changed {
            paths
                .save_projections(&projections)
                .map_err(map_registry_state)?;
        }

        let config_indices = plans
            .iter()
            .flat_map(|plan| plan.actions.iter())
            .filter(|action| action.category == "fix_config_disable" && args.fix_config)
            .filter_map(|action| action.details.get("entry_index").and_then(Value::as_u64))
            .map(|index| index as usize)
            .collect::<BTreeSet<_>>();
        let config_patch = patch_disabled_entries(&config_indices)?;
        let restart_required = plans.iter().any(|plan| plan.restart_required)
            || config_patch.restart_required
            || applied_actions.iter().any(|action| {
                action["category"].as_str().is_some_and(|category| {
                    matches!(
                        category,
                        "create_projection" | "repair_projection" | "remove_stale_projection"
                    )
                })
            });

        let op_id = record_registry_operation(
            &paths,
            "codex.reconcile",
            json!({
                "agent": CODEX_AGENT,
                "binding_id": args.binding,
                "target_id": args.target,
                "fix_config": args.fix_config,
                "request_id": request_id
            }),
            json!({
                "applied_actions": applied_actions,
                "config_patch": config_patch,
                "projection_state_changed": projection_state_changed,
                "restart_required": restart_required
            }),
        )
        .map_err(map_registry_state)?;

        let commit = commit_registry_state(&self.ctx, "codex: reconcile active-view visibility")?;
        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "codex.reconcile",
                request_id,
                json!({"commit": commit, "restart_required": restart_required}),
                &mut meta,
            )?;
        }

        Ok((
            json!({
                "dry_run": false,
                "plans": plans,
                "commit": commit,
                "restart_required": restart_required,
                "noop": applied_actions.is_empty() && !config_patch.restart_required
            }),
            meta,
        ))
    }
}

fn ensure_codex_agent(agent: AgentKind) -> std::result::Result<(), CommandFailure> {
    if agent == AgentKind::Codex {
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::ArgInvalid,
        "skill visibility currently supports only --agent codex",
    ))
}

fn reconcile_request(args: &CodexReconcileArgs, dry_run: bool) -> CodexReconcileRequest {
    CodexReconcileRequest {
        binding_id: args.binding.clone(),
        target_id: args.target.clone(),
        allowlist_path: args.allowlist.clone(),
        dry_run,
        fix_config: args.fix_config,
    }
}

fn apply_projection_repair(
    ctx: &crate::state::AppContext,
    action: &CodexReconcileAction,
    projections: &mut crate::state_model::RegistryProjectionsFile,
    head: &str,
) -> std::result::Result<(), CommandFailure> {
    let skill = action_skill(action)?;
    let target_id = action.details["target_id"].as_str().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::InternalError,
            "projection repair action is missing target_id",
        )
    })?;
    let binding_id = action.details["binding_id"].as_str().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::InternalError,
            "projection repair action is missing binding_id",
        )
    })?;
    let path = action_path(action)?;
    let source = ctx.skill_path(skill);
    if path_exists_or_symlink(&path) {
        if !path_is_symlink(&path) {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!("projection path '{}' is not a symlink", path.display()),
            ));
        }
        if !projection_path_is_safe_symlink(&path, &source)
            && action.category != "repair_projection"
        {
            return Err(CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "projection path '{}' is not a safe Loom-owned symlink",
                    path.display()
                ),
            ));
        }
        remove_symlink(&path).map_err(map_io)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    project_skill_to_target(&source, &path, ProjectionMethod::Symlink)
        .map_err(map_project_io(ProjectionMethod::Symlink))?;
    let instance_id = projection_instance_id(skill, binding_id, target_id);
    upsert_projection(
        projections,
        RegistryProjectionInstance {
            instance_id,
            skill_id: skill.to_string(),
            binding_id: Some(binding_id.to_string()),
            target_id: target_id.to_string(),
            materialized_path: path.display().to_string(),
            method: crate::core::vocab::ProjectionMethod::Symlink,
            last_applied_rev: head.to_string(),
            health: crate::core::vocab::Health::Healthy,
            observed_drift: Some(false),
            updated_at: Some(Utc::now()),
        },
    );
    Ok(())
}

fn remove_stale_projection(
    action: &CodexReconcileAction,
) -> std::result::Result<(), CommandFailure> {
    let path = action_path(action)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => remove_symlink(&path).map_err(map_io),
        Ok(_) => Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "stale projection path '{}' is not a symlink",
                path.display()
            ),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(map_io(err)),
    }
}

fn remove_stale_record(
    action: &CodexReconcileAction,
    projections: &mut crate::state_model::RegistryProjectionsFile,
) -> bool {
    let Some(instance_id) = action.details["instance_id"].as_str() else {
        return false;
    };
    let before = projections.projections.len();
    projections
        .projections
        .retain(|projection| projection.instance_id != instance_id);
    projections.projections.len() != before
}

fn action_skill(action: &CodexReconcileAction) -> std::result::Result<&str, CommandFailure> {
    action.skill.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("{} action is missing skill", action.category),
        )
    })
}

fn action_path(action: &CodexReconcileAction) -> std::result::Result<PathBuf, CommandFailure> {
    action.path.as_deref().map(PathBuf::from).ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("{} action is missing path", action.category),
        )
    })
}

fn path_is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

#[cfg(unix)]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path)
}
