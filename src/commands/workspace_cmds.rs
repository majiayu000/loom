use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::json;

use crate::cli::{
    AgentKind, BindingAddArgs, RemoteCommand, TargetAddArgs, TargetCommand, TargetOwnership,
    WorkspaceBindingCommand, WorkspaceInitArgs,
};
use crate::state::AppContext;
use crate::state_model::V3Snapshot;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::{V3StatePaths, V3WorkspaceBinding, V3WorkspaceMatcher};
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, collect_skill_inventory, map_git, map_io, map_lock, map_v3_state,
    read_git_field, record_v3_operation, remote_status_payload, remote_status_payload_with_pending,
    unique_binding_id, validate_non_empty, workspace_matcher_kind_as_str,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_status(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_inventory = collect_skill_inventory(&self.ctx);
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let pending_ops = pending_report.ops.len();
        let target_dirs = resolve_agent_skill_dirs(&self.ctx.root);
        let v3_paths = V3StatePaths::from_app_context(&self.ctx);
        let v3_status = v3_paths
            .maybe_load_snapshot()
            .map_err(map_v3_state)?
            .map(|snapshot| snapshot.status_view())
            .unwrap_or_else(|| {
                json!({
                    "state_model": "v3",
                    "available": false,
                    "error": {
                        "code": "STATE_CORRUPT",
                        "message": format!("v3 state not initialized under {}", v3_paths.v3_dir.display())
                    }
                })
            });
        let (registered_target_count, registered_target_ids) = v3_status
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

        let data = json!({
            "state_model": "v3",
            "skills": skill_inventory.source_skills,
            "backup_skills": skill_inventory.backup_skills,
            "skill_sources": {
                "dirs": skill_inventory
                    .source_dirs
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "count": skill_inventory.source_dirs.len(),
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
            "v3": v3_status
        });

        Ok((data, meta))
    }

    pub fn cmd_doctor(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let fsck = gitops::fsck(&self.ctx);
        let fsck_ok = fsck.is_ok();
        let fsck_output = fsck.unwrap_or_else(|e| e.to_string());
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let v3_paths = V3StatePaths::from_app_context(&self.ctx);
        let v3_schema_ok = v3_paths.schema_file.exists();
        let v3_snapshot = v3_paths.maybe_load_snapshot().map_err(map_v3_state)?;
        let v3_snapshot_ok = v3_snapshot.is_some();
        let history = gitops::history_status(&self.ctx).map_err(map_git)?;

        let projection_checks = v3_snapshot
            .as_ref()
            .map(|snapshot| check_projection_drift(&self.ctx, snapshot))
            .unwrap_or_default();
        let projections_ok = projection_checks
            .iter()
            .all(|check| check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));

        let healthy = fsck_ok
            && v3_schema_ok
            && v3_snapshot_ok
            && history.conflicts.is_empty()
            && projections_ok;

        Ok((
            json!({
                "healthy": healthy,
                "checks": {
                    "git_fsck": {"ok": fsck_ok, "output": fsck_output},
                    "v3_schema_file": {"ok": v3_schema_ok},
                    "v3_snapshot": {"ok": v3_snapshot_ok},
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
        self.ensure_write_repo_ready()?;
        self.ensure_v3_layout()?;

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

        Ok((
            json!({
                "initialized": true,
                "scanned": args.scan_existing,
                "imported": imported,
                "skipped": skipped,
            }),
            Meta::default(),
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
                    let snapshot = self.require_v3_snapshot()?;
                    json!({
                        "state_model": "v3",
                        "count": snapshot.bindings.bindings.len(),
                        "bindings": snapshot.bindings.bindings
                    })
                },
                Meta::default(),
            )),
            WorkspaceBindingCommand::Show(args) => {
                let snapshot = self.require_v3_snapshot()?;
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
                        "state_model": "v3",
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
        self.ensure_write_layout()?;
        validate_non_empty("profile", &args.profile)?;
        validate_non_empty("matcher_value", &args.matcher_value)?;
        validate_non_empty("target", &args.target)?;
        validate_non_empty("policy_profile", &args.policy_profile)?;

        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;
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
        let binding = V3WorkspaceBinding {
            binding_id: binding_id.clone(),
            agent: agent_kind_as_str(args.agent).to_string(),
            profile_id: args.profile.clone(),
            workspace_matcher: V3WorkspaceMatcher {
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
        paths.save_bindings(&bindings).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
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
        .map_err(map_v3_state)?;

        Ok((
            json!({"binding": binding, "noop": false}),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    fn cmd_workspace_binding_remove(
        &self,
        args: &crate::cli::BindingShowArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let paths = self.ensure_v3_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_v3_state)?;
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
        snapshot
            .projections
            .projections
            .retain(|item| item.binding_id != args.binding_id);

        paths
            .save_bindings_rules_projections(
                &snapshot.bindings,
                &snapshot.rules,
                &snapshot.projections,
            )
            .map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "workspace.binding.remove",
            json!({
                "binding_id": binding.binding_id,
                "request_id": request_id
            }),
            json!({
                "binding_id": binding.binding_id,
                "removed_rules": removed_rules.iter().map(|rule| rule.skill_id.clone()).collect::<Vec<_>>(),
                "removed_projection_ids": removed_projections.iter().map(|projection| projection.instance_id.clone()).collect::<Vec<_>>(),
            }),
        )
        .map_err(map_v3_state)?;

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if !orphaned_paths.is_empty() {
            meta.warnings.push(format!(
                "binding removed from state; {} live projection path(s) were left in place",
                orphaned_paths.len()
            ));
        }

        Ok((
            json!({
                "binding": binding,
                "removed_rules": removed_rules,
                "removed_projections": removed_projections,
                "orphaned_paths": orphaned_paths,
                "noop": false
            }),
            meta,
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

fn check_projection_drift(ctx: &AppContext, snapshot: &V3Snapshot) -> Vec<serde_json::Value> {
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
