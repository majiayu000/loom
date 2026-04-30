use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;

use crate::cli::{
    AgentKind, BindingAddArgs, OrphanCleanArgs, RemoteCommand, TargetAddArgs, TargetCommand,
    TargetOwnership, WorkspaceBindingCommand, WorkspaceInitArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::{
    RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths, RegistryWorkspaceBinding,
    RegistryWorkspaceMatcher,
};
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, collect_skill_inventory, commit_registry_state, map_git, map_io, map_lock,
    map_registry_state, maybe_autosync_or_queue, read_git_field, record_registry_operation,
    remote_status_payload, remote_status_payload_with_pending, unique_binding_id,
    validate_non_empty, workspace_matcher_kind_as_str,
};
use super::{App, CommandFailure};

enum LivePathCleanup {
    Deleted(String),
    Skipped { path: String, reason: &'static str },
}

fn cleanup_orphan_live_path(
    projection: &RegistryProjectionInstance,
    target_paths: &HashMap<String, String>,
) -> std::result::Result<LivePathCleanup, CommandFailure> {
    let path = Path::new(&projection.materialized_path);
    if !path.exists() {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_missing",
        });
    }

    let Some(target_path) = target_paths.get(&projection.target_id) else {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "target_not_registered",
        });
    };

    let metadata = fs::symlink_metadata(path).map_err(map_io)?;
    if metadata.file_type().is_symlink() {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_is_symlink",
        });
    }
    if !metadata.is_dir() {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_not_directory",
        });
    }

    let target_root = match fs::canonicalize(PathBuf::from(target_path)) {
        Ok(root) => root,
        Err(_) => {
            return Ok(LivePathCleanup::Skipped {
                path: projection.materialized_path.clone(),
                reason: "target_path_missing",
            });
        }
    };
    let live_path = fs::canonicalize(path).map_err(map_io)?;

    if live_path == target_root {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_is_target_root",
        });
    }
    if !live_path.starts_with(&target_root) {
        return Ok(LivePathCleanup::Skipped {
            path: projection.materialized_path.clone(),
            reason: "path_outside_target",
        });
    }

    fs::remove_dir_all(path).map_err(map_io)?;
    Ok(LivePathCleanup::Deleted(
        projection.materialized_path.clone(),
    ))
}

impl App {
    pub fn cmd_status(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_inventory = collect_skill_inventory(&self.ctx);
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let pending_ops = pending_report.ops.len();
        let target_dirs = resolve_agent_skill_dirs(&self.ctx.root);
        let registry_paths = RegistryStatePaths::from_app_context(&self.ctx);
        let registry_status = registry_paths
            .maybe_load_snapshot()
            .map_err(map_registry_state)?
            .map(|snapshot| snapshot.status_view())
            .unwrap_or_else(|| {
                json!({
                    "state_model": "registry",
                    "available": false,
                    "error": {
                        "code": "STATE_CORRUPT",
                        "message": format!("registry state not initialized under {}", registry_paths.registry_dir.display())
                    }
                })
            });
        let (registered_target_count, registered_target_ids) = registry_status
            .get("targets")
            .and_then(|value| value.as_array())
            .map(|targets| {
                let ids = targets
                    .iter()
                    .filter_map(|target| target.get("target_id").and_then(|id| id.as_str()))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>();
                (targets.len(), ids)
            })
            .unwrap_or((0, Vec::new()));
        let mut git_warnings = Vec::new();
        let head = read_git_field(&self.ctx, &["rev-parse", "HEAD"], &mut git_warnings);
        let branch = read_git_field(
            &self.ctx,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            &mut git_warnings,
        );
        let status_short = read_git_field(&self.ctx, &["status", "--short"], &mut git_warnings);

        let (remote, mut meta) = remote_status_payload_with_pending(&self.ctx, pending_report)?;
        meta.warnings.splice(0..0, git_warnings);
        meta.warnings.extend(skill_inventory.warnings);
        let source_skill_sample = skill_inventory
            .source_skills
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>();

        let data = json!({
            "state_model": "registry",
            "inventory": {
                "source_skill_count": skill_inventory.source_skills.len(),
                "source_skill_sample": source_skill_sample,
                "source_skill_sample_truncated": skill_inventory.source_skills.len() > 20,
                "backup_skill_count": skill_inventory.backup_skills.len(),
                "source_dirs": skill_inventory
                        .source_dirs
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>(),
            },
            "backup_dir": self.ctx.skills_dir.display().to_string(),
            "git": {"head": head, "branch": branch, "status_short": status_short},
            "agent_dir_defaults": {
                "claude_dir": target_dirs.claude.display().to_string(),
                "codex_dir": target_dirs.codex.display().to_string()
            },
            "registered_targets": {
                "count": registered_target_count,
                "target_ids": registered_target_ids
            },
            "remote": remote,
            "pending_ops": pending_ops,
            "registry": registry_status
        });

        Ok((data, meta))
    }

    pub fn cmd_doctor(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let fsck = gitops::fsck(&self.ctx);
        let fsck_ok = fsck.is_ok();
        let fsck_output = fsck.unwrap_or_else(|e| e.to_string());
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let registry_paths = RegistryStatePaths::from_app_context(&self.ctx);
        let registry_schema_ok = registry_paths.schema_file.exists();
        let registry_snapshot = registry_paths
            .maybe_load_snapshot()
            .map_err(map_registry_state)?;
        let registry_snapshot_ok = registry_snapshot.is_some();
        let history = gitops::history_status(&self.ctx).map_err(map_git)?;

        let projection_checks = registry_snapshot
            .as_ref()
            .map(|snapshot| check_projection_drift(&self.ctx, snapshot))
            .unwrap_or_default();
        let projections_ok = projection_checks
            .iter()
            .all(|check| check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));

        let healthy = fsck_ok
            && registry_schema_ok
            && registry_snapshot_ok
            && history.conflicts.is_empty()
            && projections_ok;

        Ok((
            json!({
                "healthy": healthy,
                "checks": {
                    "git_fsck": {"ok": fsck_ok, "output": fsck_output},
                    "registry_schema_file": {"ok": registry_schema_ok},
                    "registry_snapshot": {"ok": registry_snapshot_ok},
                    "pending_queue": {
                        "count": pending_report.ops.len(),
                        "journal_events": pending_report.journal_events,
                        "history_events": pending_report.history_events,
                        "warnings": pending_report.warnings
                    },
                    "history_branch": history,
                    "projection_drift": {
                        "ok": projections_ok,
                        "projections": projection_checks
                    }
                }
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_workspace_init(
        &self,
        args: &WorkspaceInitArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        // Hold the workspace lock for the entire init, including the scan.
        // lock_workspace is reentrant within the same thread, so cmd_target
        // calls below can acquire it again without deadlock.
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.ensure_registry_layout()?;

        let mut imported: Vec<serde_json::Value> = Vec::new();
        let mut skipped: Vec<serde_json::Value> = Vec::new();

        if args.scan_existing {
            let home = std::env::var("HOME").map_err(|_| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--scan-existing requires HOME to be set",
                )
            })?;
            let candidates = [
                (AgentKind::Claude, format!("{}/.claude/skills", home)),
                (AgentKind::Codex, format!("{}/.codex/skills", home)),
            ];
            for (agent, path_str) in candidates {
                let p = Path::new(&path_str);
                if !p.exists() {
                    skipped.push(json!({
                        "agent": agent_kind_as_str(agent),
                        "path": path_str,
                        "reason": "does-not-exist",
                    }));
                    continue;
                }
                if !p.is_dir() {
                    skipped.push(json!({
                        "agent": agent_kind_as_str(agent),
                        "path": path_str,
                        "reason": "not-a-directory",
                    }));
                    continue;
                }
                let add_args = TargetAddArgs {
                    agent,
                    path: path_str.clone(),
                    ownership: TargetOwnership::Observed,
                };
                let (value, _meta) = self.cmd_target(&TargetCommand::Add(add_args), request_id)?;
                imported.push(value);
            }
        }

        let commit = commit_registry_state(&self.ctx, "workspace: initialize registry state")?;
        let mut meta = Meta::default();
        if let Some(commit) = &commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "workspace.init",
                request_id,
                json!({"commit": commit, "scanned": args.scan_existing}),
                &mut meta,
            )?;
        }

        Ok((
            json!({
                "initialized": true,
                "scanned": args.scan_existing,
                "imported": imported,
                "skipped": skipped,
                "commit": commit,
            }),
            meta,
        ))
    }

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
        validate_non_empty("policy_profile", &args.policy_profile)?;

        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
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
            agent: agent_kind_as_str(args.agent).to_string(),
            profile_id: args.profile.clone(),
            workspace_matcher: RegistryWorkspaceMatcher {
                kind: workspace_matcher_kind_as_str(args.matcher_kind).to_string(),
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

        let op_id = record_registry_operation(
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
        )
        .map_err(map_registry_state)?;
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

    fn cmd_workspace_binding_remove(
        &self,
        args: &crate::cli::BindingShowArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let binding = snapshot.binding(&args.binding_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{}' not found", args.binding_id),
            )
        })?;

        let removed_rules = snapshot.binding_rules(&args.binding_id);
        let removed_projections = snapshot.binding_projections(&args.binding_id);
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
                proj.health = "orphaned".to_string();
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

        let op_id = record_registry_operation(
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
            }),
        )
        .map_err(map_registry_state)?;

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
                "binding removed; {} projection(s) marked orphaned — run `loom skill orphan clean` to remove metadata",
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
                "commit": commit,
                "noop": false
            }),
            meta,
        ))
    }

    pub fn cmd_skill_orphan_clean(
        &self,
        args: &OrphanCleanArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let paths = self.ensure_registry_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let target_paths = snapshot
            .targets
            .targets
            .iter()
            .map(|target| (target.target_id.clone(), target.path.clone()))
            .collect::<HashMap<_, _>>();

        let mut cleaned_ids: Vec<String> = Vec::new();
        let mut deleted_paths: Vec<String> = Vec::new();
        let mut skipped_paths: Vec<serde_json::Value> = Vec::new();
        let mut retained = Vec::new();

        for proj in snapshot.projections.projections.drain(..) {
            if proj.binding_id.is_none() && proj.health == "orphaned" {
                if args.delete_live_paths {
                    match cleanup_orphan_live_path(&proj, &target_paths)? {
                        LivePathCleanup::Deleted(path) => deleted_paths.push(path),
                        LivePathCleanup::Skipped { path, reason } => {
                            skipped_paths.push(json!({
                                "projection_id": proj.instance_id.clone(),
                                "path": path,
                                "reason": reason,
                            }));
                        }
                    }
                } else if Path::new(&proj.materialized_path).exists() {
                    skipped_paths.push(json!({
                        "projection_id": proj.instance_id.clone(),
                        "path": proj.materialized_path.clone(),
                        "reason": "delete_live_paths_not_requested",
                    }));
                }
                cleaned_ids.push(proj.instance_id.clone());
            } else {
                retained.push(proj);
            }
        }
        snapshot.projections.projections = retained;

        paths
            .save_projections(&snapshot.projections)
            .map_err(map_registry_state)?;

        let op_id = record_registry_operation(
            &paths,
            "skill.orphan.clean",
            json!({ "request_id": request_id }),
            json!({
                "cleaned_projection_ids": cleaned_ids,
                "cleaned_paths": deleted_paths,
                "deleted_paths": deleted_paths,
                "skipped_paths": skipped_paths,
                "delete_live_paths": args.delete_live_paths,
            }),
        )
        .map_err(map_registry_state)?;

        Ok((
            json!({
                "cleaned_count": cleaned_ids.len(),
                "cleaned_projection_ids": cleaned_ids,
                "cleaned_paths": deleted_paths,
                "deleted_paths": deleted_paths,
                "skipped_paths": skipped_paths,
                "delete_live_paths": args.delete_live_paths,
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_remote(
        &self,
        command: &RemoteCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            RemoteCommand::Set { url } => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                gitops::set_remote_origin(&self.ctx, url).map_err(map_git)?;
                Ok((json!({"remote": "origin", "url": url}), Meta::default()))
            }
            RemoteCommand::Status => {
                let (remote, meta) = remote_status_payload(&self.ctx)?;
                Ok((json!({"remote": remote}), meta))
            }
        }
    }
}

fn check_projection_drift(ctx: &AppContext, snapshot: &RegistrySnapshot) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    for projection in &snapshot.projections.projections {
        let materialized = Path::new(&projection.materialized_path);
        let skill_src = ctx.skill_path(&projection.skill_id);
        let mut issues: Vec<&str> = Vec::new();

        if !materialized.exists() {
            issues.push("materialized_path does not exist");
        }
        if !skill_src.exists() {
            issues.push("source skill not found in registry");
        }

        if projection.method == "symlink" && materialized.exists() {
            match fs::read_link(materialized) {
                Ok(link_target) => {
                    // Relative symlink targets resolve against the symlink's parent
                    // directory, NOT the process CWD. `Path::exists` and
                    // `fs::canonicalize` both fall back to CWD for relative inputs,
                    // so a valid relative projection (e.g. `../../skills/foo`)
                    // would otherwise be reported as dangling/wrong-target.
                    let resolved = if link_target.is_absolute() {
                        link_target.clone()
                    } else {
                        materialized
                            .parent()
                            .map(|parent| parent.join(&link_target))
                            .unwrap_or_else(|| link_target.clone())
                    };
                    if !resolved.exists() {
                        issues.push("symlink target does not exist (dangling)");
                    } else {
                        let canon_link = fs::canonicalize(&resolved).ok();
                        let canon_src = fs::canonicalize(&skill_src).ok();
                        if canon_link != canon_src {
                            issues.push("symlink points to wrong target");
                        }
                    }
                }
                Err(_) => {
                    if materialized.exists() {
                        issues.push("expected symlink but path is not a symlink");
                    }
                }
            }
        }

        results.push(json!({
            "instance_id": projection.instance_id,
            "skill_id": projection.skill_id,
            "method": projection.method,
            "ok": issues.is_empty(),
            "issues": issues,
        }));
    }
    results
}

#[cfg(test)]
mod orphan_tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    use chrono::Utc;

    use crate::state::AppContext;
    use crate::state_model::{
        REGISTRY_SCHEMA_VERSION, RegistryBindingsFile, RegistryOpsCheckpoint,
        RegistryProjectionInstance, RegistryProjectionTarget, RegistryProjectionsFile,
        RegistryRulesFile, RegistrySchemaFile, RegistryStatePaths, RegistryTargetCapabilities,
        RegistryTargetsFile, RegistryWorkspaceBinding, RegistryWorkspaceMatcher,
    };

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-orphan-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_minimal_registry(root: &Path) -> RegistryStatePaths {
        let paths = RegistryStatePaths::from_root(root);
        paths.ensure_layout().unwrap();
        let now = Utc::now();
        fs::write(
            &paths.schema_file,
            serde_json::to_vec_pretty(&RegistrySchemaFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                created_at: now,
                writer: "test".into(),
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.targets_file,
            serde_json::to_vec_pretty(&RegistryTargetsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                targets: vec![RegistryProjectionTarget {
                    target_id: "target1".into(),
                    agent: "claude".into(),
                    path: root.display().to_string(),
                    ownership: "registered".into(),
                    capabilities: RegistryTargetCapabilities {
                        symlink: false,
                        copy: true,
                        watch: true,
                    },
                    created_at: None,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.rules_file,
            serde_json::to_vec_pretty(&RegistryRulesFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                rules: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.checkpoint_file,
            serde_json::to_vec_pretty(&RegistryOpsCheckpoint {
                schema_version: REGISTRY_SCHEMA_VERSION,
                last_scanned_op_id: None,
                last_acked_op_id: None,
                updated_at: now,
            })
            .unwrap(),
        )
        .unwrap();
        paths
    }

    fn make_binding(id: &str) -> RegistryWorkspaceBinding {
        RegistryWorkspaceBinding {
            binding_id: id.into(),
            agent: "claude".into(),
            profile_id: "default".into(),
            workspace_matcher: RegistryWorkspaceMatcher {
                kind: "name".into(),
                value: "test".into(),
            },
            default_target_id: "target1".into(),
            policy_profile: "safe-capture".into(),
            active: true,
            created_at: None,
        }
    }

    fn make_projection(
        instance_id: &str,
        binding_id: &str,
        health: &str,
        mat_path: &str,
    ) -> RegistryProjectionInstance {
        RegistryProjectionInstance {
            instance_id: instance_id.into(),
            skill_id: "skill1".into(),
            binding_id: Some(binding_id.into()),
            target_id: "target1".into(),
            materialized_path: mat_path.into(),
            method: "copy".into(),
            last_applied_rev: "abc123".into(),
            health: health.into(),
            observed_drift: Some(false),
            updated_at: None,
        }
    }

    fn orphan_clean_args(delete_live_paths: bool) -> crate::cli::OrphanCleanArgs {
        crate::cli::OrphanCleanArgs { delete_live_paths }
    }

    fn setup_with_binding_and_projection(
        root: &Path,
        mat_path: &str,
        health: &str,
    ) -> RegistryStatePaths {
        let paths = write_minimal_registry(root);

        fs::write(
            &paths.bindings_file,
            serde_json::to_vec_pretty(&RegistryBindingsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                bindings: vec![make_binding("bind1")],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.projections_file,
            serde_json::to_vec_pretty(&RegistryProjectionsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                projections: vec![make_projection("inst1", "bind1", health, mat_path)],
            })
            .unwrap(),
        )
        .unwrap();
        crate::gitops::ensure_repo_initialized(&AppContext::new(Some(root.to_path_buf())).unwrap())
            .ok();
        paths
    }

    #[test]
    fn binding_removal_marks_projection_orphaned_not_deleted() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req-test",
        )
        .unwrap();

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        assert_eq!(
            snapshot.projections.projections.len(),
            1,
            "projection must not be deleted"
        );
        let proj = &snapshot.projections.projections[0];
        assert_eq!(proj.health, "orphaned");
        assert!(proj.binding_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn binding_removal_effects_use_orphaned_projection_ids_key() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        let (data, _meta) = app
            .cmd_workspace_binding_remove(
                &crate::cli::BindingShowArgs {
                    binding_id: "bind1".into(),
                },
                "req-test",
            )
            .unwrap();

        let ids = data["orphaned_projection_ids"]
            .as_array()
            .expect("orphaned_projection_ids array");
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], "inst1");
        assert!(
            data.get("removed_projection_ids").is_none(),
            "old key must not appear"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn binding_removal_orphans_drifted_projection() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "drifted");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req-test",
        )
        .unwrap();

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        let proj = &snapshot.projections.projections[0];
        assert_eq!(proj.health, "orphaned");
        assert!(proj.binding_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn binding_removal_orphans_conflict_projection() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "conflict");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req-test",
        )
        .unwrap();

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        let proj = &snapshot.projections.projections[0];
        assert_eq!(proj.health, "orphaned");
        assert!(proj.binding_id.is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn orphan_clean_removes_metadata_without_deleting_paths_by_default() {
        let root = temp_root();
        let mat_dir = root.join("mat_proj");
        fs::create_dir_all(&mat_dir).unwrap();
        let mat_path = mat_dir.display().to_string();

        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        // First orphan the projection via binding removal
        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req1",
        )
        .unwrap();

        // Now clean orphans
        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (data, _meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(false), "req2")
            .unwrap();

        assert_eq!(data["cleaned_count"], 1);
        assert_eq!(data["deleted_paths"].as_array().unwrap().len(), 0);
        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        assert!(
            snapshot.projections.projections.is_empty(),
            "orphaned record must be removed"
        );
        assert!(
            mat_dir.exists(),
            "materialized path must be preserved by default"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn orphan_clean_deletes_live_path_only_with_explicit_flag() {
        let root = temp_root();
        let mat_dir = root.join("mat_proj");
        fs::create_dir_all(&mat_dir).unwrap();
        let mat_path = mat_dir.display().to_string();

        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req1",
        )
        .unwrap();

        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (data, _meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(true), "req2")
            .unwrap();

        assert_eq!(data["cleaned_count"], 1);
        assert_eq!(data["deleted_paths"].as_array().unwrap().len(), 1);
        assert!(!mat_dir.exists(), "validated live path must be deleted");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn orphan_clean_refuses_to_delete_paths_outside_registered_target() {
        let root = temp_root();
        let outside = temp_root();
        let mat_dir = outside.join("mat_proj");
        fs::create_dir_all(&mat_dir).unwrap();
        let mat_path = mat_dir.display().to_string();

        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req1",
        )
        .unwrap();

        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (data, _meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(true), "req2")
            .unwrap();

        assert_eq!(data["cleaned_count"], 1);
        assert!(mat_dir.exists(), "outside path must not be deleted");
        assert_eq!(
            data["skipped_paths"][0]["reason"],
            serde_json::Value::String("path_outside_target".into())
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn orphan_clean_audit_records_skill_orphan_clean_intent() {
        let root = temp_root();
        let mat_path = root.join("proj1").display().to_string();
        setup_with_binding_and_projection(&root, &mat_path, "healthy");

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req1",
        )
        .unwrap();

        let ctx2 = AppContext::new(Some(root.clone())).unwrap();
        let app2 = crate::commands::App { ctx: ctx2 };
        let (_data, meta) = app2
            .cmd_skill_orphan_clean(&orphan_clean_args(false), "req2")
            .unwrap();
        assert!(meta.op_id.is_some(), "op_id must be recorded");

        let paths = RegistryStatePaths::from_root(&root);
        let snapshot = paths.load_snapshot().unwrap();
        let clean_op = snapshot
            .operations
            .iter()
            .find(|op| op.intent == "skill.orphan.clean")
            .expect("skill.orphan.clean op must exist");
        assert!(
            clean_op.effects["cleaned_projection_ids"]
                .as_array()
                .is_some()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn removing_one_binding_does_not_orphan_other_binding_projections() {
        let root = temp_root();
        let paths = write_minimal_registry(&root);
        let now = Utc::now();

        fs::write(
            &paths.bindings_file,
            serde_json::to_vec_pretty(&RegistryBindingsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                bindings: vec![make_binding("bind1"), make_binding("bind2")],
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &paths.projections_file,
            serde_json::to_vec_pretty(&RegistryProjectionsFile {
                schema_version: REGISTRY_SCHEMA_VERSION,
                projections: vec![
                    make_projection("inst1", "bind1", "healthy", "/tmp/p1"),
                    make_projection("inst2", "bind2", "healthy", "/tmp/p2"),
                ],
            })
            .unwrap(),
        )
        .unwrap();
        crate::gitops::ensure_repo_initialized(&AppContext::new(Some(root.clone())).unwrap()).ok();
        let _ = now;

        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let app = crate::commands::App { ctx };
        app.cmd_workspace_binding_remove(
            &crate::cli::BindingShowArgs {
                binding_id: "bind1".into(),
            },
            "req-test",
        )
        .unwrap();

        let snap = RegistryStatePaths::from_root(&root)
            .load_snapshot()
            .unwrap();
        assert_eq!(snap.projections.projections.len(), 2);
        let inst2 = snap
            .projections
            .projections
            .iter()
            .find(|p| p.instance_id == "inst2")
            .unwrap();
        assert_eq!(inst2.health, "healthy");
        assert_eq!(inst2.binding_id.as_deref(), Some("bind2"));

        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn unique_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loom-symlink-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Regression for PR #1 review: `check_projection_drift` previously called
    /// `link_target.exists()` and `fs::canonicalize(&link_target)` on the raw
    /// `read_link` result, which resolves relative paths against the process
    /// CWD instead of the symlink's parent directory. A valid relative
    /// projection (e.g. `../skills/foo`) was therefore mis-reported as
    /// dangling/wrong-target. This test mirrors the production resolution
    /// rule and asserts it canonicalizes to the actual source.
    #[test]
    fn relative_symlink_resolves_against_parent_directory() {
        let base = unique_temp_dir();
        let src = base.join("skill_src");
        fs::create_dir(&src).unwrap();
        let materialized = base.join("link");
        std::os::unix::fs::symlink("skill_src", &materialized).unwrap();

        let link_target = fs::read_link(&materialized).unwrap();
        assert!(link_target.is_relative(), "fixture must be a relative link");

        let resolved = if link_target.is_absolute() {
            link_target.clone()
        } else {
            materialized
                .parent()
                .map(|parent| parent.join(&link_target))
                .unwrap()
        };

        assert!(resolved.exists(), "resolved relative link must exist");
        let canon_link = fs::canonicalize(&resolved).unwrap();
        let canon_src = fs::canonicalize(&src).unwrap();
        assert_eq!(canon_link, canon_src);

        let _ = fs::remove_dir_all(&base);
    }
}
