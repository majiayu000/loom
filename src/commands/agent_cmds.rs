use std::path::{Path, PathBuf};

mod planning_helpers;

use serde_json::{Value, json};

use super::codex_reconcile_plan::plan_agent_reconcile;
use super::codex_visibility::CodexReconcileRequest;
use super::helpers::{
    agent_kind_as_str, map_arg, map_git, map_io, projection_method_as_str, shell_arg,
    validate_skill_name,
};
use super::skill_safety::evaluate_skill_safety_with_policy;
use super::{App, CommandFailure};
use crate::agent_adapters::{AgentAdapter, built_in_adapter_for_agent, load_agent_adapters};
use crate::cli::{
    AgentPreflightArgs, AgentReconcileArgs, OrphanCleanArgs, ProjectArgs, RollbackArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use planning_helpers::{
    build_preflight_next_commands, is_orphan_projection, is_safe, normalize_path,
    push_target_risks, risk, rollback_impacted_projections, status_for, target_paths,
    workspace_matches,
};

const ROLLBACK_PREVIEW_PATH_LIMIT: usize = 50;

impl App {
    pub fn cmd_agent_preflight(
        &self,
        args: &AgentPreflightArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if let Some(skill) = args.skill.as_deref() {
            validate_skill_name(skill).map_err(map_arg)?;
        }
        let snapshot = self.require_registry_snapshot()?;
        let agent = agent_kind_as_str(args.agent);
        let workspace = normalize_path(&args.workspace);
        let mut risks = Vec::new();
        let mut matches = Vec::new();

        for binding in snapshot.bindings.bindings.iter().filter(|binding| {
            binding.active
                && binding.agent == agent
                && workspace_matches(
                    binding.workspace_matcher.kind.as_str(),
                    &binding.workspace_matcher.value,
                    &workspace,
                )
        }) {
            let matching_rules = args
                .skill
                .as_deref()
                .map(|skill| {
                    snapshot
                        .rules
                        .rules
                        .iter()
                        .filter(|rule| {
                            rule.binding_id == binding.binding_id && rule.skill_id == skill
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let (method, target_id) = match matching_rules.as_slice() {
                [] => (args.method, Some(binding.default_target_id.as_str())),
                [rule] => (rule.method, Some(rule.target_id.as_str())),
                rules => {
                    let target_ids = rules
                        .iter()
                        .map(|rule| rule.target_id.as_str())
                        .collect::<Vec<_>>();
                    risks.push(risk(
                        "error",
                        "AMBIGUOUS_SKILL_RULE",
                        format!(
                            "binding '{}' has {} '{}' rules for targets {}; use an explicit target selector before projecting this skill",
                            binding.binding_id,
                            rules.len(),
                            args.skill.as_deref().unwrap_or_default(),
                            target_ids.join(", ")
                        ),
                    ));
                    (args.method, None)
                }
            };
            let target = target_id.and_then(|target_id| snapshot.target(target_id));
            if let Some(target) = target {
                push_target_risks(
                    &mut risks,
                    &snapshot,
                    &binding.binding_id,
                    &target.target_id,
                    method.as_str(),
                );
            } else if let Some(target_id) = target_id {
                risks.push(risk(
                    "error",
                    "TARGET_NOT_FOUND",
                    format!(
                        "binding '{}' points at missing target '{}'",
                        binding.binding_id, target_id
                    ),
                ));
            }
            matches.push(json!({
                "binding_id": binding.binding_id,
                "agent": binding.agent,
                "profile": binding.profile_id,
                "matcher": binding.workspace_matcher,
                "target_id": target_id,
                "target": target,
                "method": method,
                "existing_projection": args.skill.as_deref().and_then(|skill| {
                    let target_id = target_id?;
                    snapshot.projections.projections.iter().find(|projection| {
                        projection.skill_id == skill
                            && projection.binding_id.as_deref() == Some(binding.binding_id.as_str())
                            && projection.target_id == target_id
                    })
                }),
            }));
        }

        match matches.len() {
            0 => risks.push(risk(
                "error",
                "NO_MATCHING_BINDING",
                format!(
                    "no active '{}' binding matches workspace '{}'",
                    agent,
                    workspace.display()
                ),
            )),
            1 => {}
            count => risks.push(risk(
                "error",
                "AMBIGUOUS_BINDING",
                format!(
                    "{} active '{}' bindings match workspace '{}'; refine workspace binding matchers or use the returned binding_id with the write command",
                    count,
                    agent,
                    workspace.display()
                ),
            )),
        }

        if let Some(skill) = args.skill.as_deref()
            && !self.ctx.skill_path(skill).exists()
        {
            risks.push(risk(
                "error",
                "SKILL_NOT_FOUND",
                format!("skill '{}' not found", skill),
            ));
        }

        let required_selectors = if matches.len() == 1 {
            let binding_id = matches[0]["binding_id"].as_str().unwrap_or_default();
            json!({
                "skill": args.skill,
                "binding_id": binding_id,
                "target_id": matches[0]["target_id"],
                "method": matches[0]["method"],
            })
        } else {
            json!({
                "skill": args.skill,
                "binding_id": null,
                "target_id": null,
                "method": projection_method_as_str(args.method),
            })
        };
        let next_commands =
            build_preflight_next_commands(&self.ctx.root, args, &required_selectors);

        Ok((
            json!({
                "dry_run": true,
                "operation": "agent.preflight",
                "safe_to_run": is_safe(&risks),
                "status": status_for(&risks, matches.len()),
                "workspace": workspace.display().to_string(),
                "agent": agent,
                "required_selectors": required_selectors,
                "target_paths": target_paths(&matches),
                "matches": matches,
                "risks": risks,
                "next_commands": next_commands,
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_agent_reconcile(
        &self,
        args: &AgentReconcileArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let snapshot = self.require_registry_snapshot()?;
        let agent = agent_kind_as_str(args.agent).to_string();
        if let Some(check) = reconcile_visibility_unsupported_check(&self.ctx, &agent)? {
            return Ok((
                json!({"dry_run": true, "plans": [], "checks": [check], "unsupported": true}),
                Meta::default(),
            ));
        }
        let request = CodexReconcileRequest {
            agent,
            binding_id: args.binding.clone(),
            target_id: args.target.clone(),
            allowlist_path: args.allowlist.clone(),
            dry_run: true,
            fix_config: false,
        };
        let plans = plan_agent_reconcile(&self.ctx, &snapshot, &request)?;
        Ok((json!({"dry_run": true, "plans": plans}), Meta::default()))
    }

    pub fn cmd_project_plan(
        &self,
        args: &ProjectArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let snapshot = self.require_registry_snapshot()?;
        let mut risks = Vec::new();

        if !self.ctx.skill_path(&args.skill).exists() {
            risks.push(risk(
                "error",
                "SKILL_NOT_FOUND",
                format!("skill '{}' not found", args.skill),
            ));
        }

        let binding = snapshot.binding(&args.binding);
        let mut target_id = args.target.clone();
        if let Some(binding) = binding {
            if target_id.is_none() {
                target_id = Some(binding.default_target_id.clone());
            }
        } else {
            risks.push(risk(
                "error",
                "BINDING_NOT_FOUND",
                format!("binding '{}' not found", args.binding),
            ));
        }

        let target = target_id.as_deref().and_then(|id| snapshot.target(id));
        if let Some(target) = target {
            if let Some(binding) = binding
                && target.agent != binding.agent
            {
                risks.push(risk(
                    "error",
                    "TARGET_AGENT_MISMATCH",
                    format!(
                        "binding '{}' is for agent '{}' but target '{}' is for agent '{}'",
                        binding.binding_id, binding.agent, target.target_id, target.agent
                    ),
                ));
            }
            push_target_risks(
                &mut risks,
                &snapshot,
                binding
                    .map(|b| b.binding_id.as_str())
                    .unwrap_or(args.binding.as_str()),
                &target.target_id,
                projection_method_as_str(args.method),
            );
        } else if let Some(target_id) = target_id.as_deref() {
            risks.push(risk(
                "error",
                "TARGET_NOT_FOUND",
                format!("target '{}' not found", target_id),
            ));
        }

        let materialized_path = target.map(|target| PathBuf::from(&target.path).join(&args.skill));
        if let Some(path) = materialized_path.as_ref()
            && path.exists()
        {
            risks.push(risk(
                "warning",
                "REPLACE_EXISTING_PROJECTION",
                format!(
                    "projection path '{}' already exists and would be replaced",
                    path.display()
                ),
            ));
        }

        let mut safety_report = None;
        let mut policy_report = None;
        if self.ctx.skill_path(&args.skill).exists() {
            let profile = binding
                .map(|binding| binding.policy_profile.as_str())
                .unwrap_or("safe-capture");
            match evaluate_skill_safety_with_policy(
                &self.ctx,
                &args.skill,
                "activate",
                false,
                profile,
            ) {
                Ok(evaluation) => {
                    let report = evaluation.report;
                    if !report.activation_allowed {
                        risks.push(risk(
                            "error",
                            "POLICY_BLOCKED",
                            format!(
                                "safety decision '{}' would block projection of skill '{}'",
                                report.decision, args.skill
                            ),
                        ));
                    } else if report.summary.high + report.summary.critical > 0 {
                        risks.push(risk(
                            "warning",
                            "POLICY_WARNINGS",
                            format!(
                                "safety scan reported {} high-risk finding(s)",
                                report.summary.high + report.summary.critical
                            ),
                        ));
                    }
                    policy_report = Some(evaluation.policy);
                    safety_report = Some(report);
                }
                Err(err) => risks.push(risk("error", err.code.as_str(), err.message)),
            }
        }

        let mut next_command = format!(
            "loom --json --root {} skill project {} --binding {} --method {}",
            shell_arg(&self.ctx.root),
            shell_arg(&args.skill),
            shell_arg(&args.binding),
            projection_method_as_str(args.method)
        );
        if let Some(target_id) = target_id.as_deref() {
            next_command.push_str(&format!(" --target {}", shell_arg(target_id)));
        }

        Ok((
            json!({
                "dry_run": true,
                "operation": "skill.project",
                "safe_to_run": is_safe(&risks),
                "status": status_for(&risks, usize::from(binding.is_some())),
                "required_selectors": {
                    "skill": args.skill,
                    "binding_id": args.binding,
                    "target_id": target_id,
                    "method": projection_method_as_str(args.method),
                },
                "target_paths": materialized_path.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                "will_mutate": ["live_target", "registry_state", "registry_ops", "git_history"],
                "policy": policy_report,
                "safety": safety_report,
                "risks": risks,
                "next_commands": [next_command],
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_rollback_plan(
        &self,
        args: &RollbackArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let mut risks = Vec::new();
        let mut warnings = Vec::new();
        if args.to.is_some() && args.steps.is_some() {
            risks.push(risk(
                "error",
                "ARG_INVALID",
                "--to and --steps are mutually exclusive",
            ));
        }
        let skill_exists = self.ctx.skill_path(&args.skill).exists();
        if !skill_exists {
            risks.push(risk(
                "error",
                "SKILL_NOT_FOUND",
                format!("skill '{}' not found", args.skill),
            ));
        }
        let reference = match (&args.to, args.steps) {
            (Some(r), _) => r.clone(),
            (None, Some(n)) => format!("HEAD~{}", n),
            (None, None) => "HEAD~1".to_string(),
        };
        let current_commit = match gitops::head(&self.ctx) {
            Ok(rev) => Some(rev),
            Err(err) => {
                risks.push(risk(
                    "error",
                    "GIT_ERROR",
                    format!("failed to resolve current HEAD: {}", err),
                ));
                None
            }
        };
        let target_commit = match gitops::resolve_ref(&self.ctx, &reference) {
            Ok(rev) => Some(rev),
            Err(err) => {
                risks.push(risk(
                    "error",
                    "GIT_ERROR",
                    format!("failed to resolve '{}': {}", reference, err),
                ));
                None
            }
        };
        let skill_rel = format!("skills/{}", args.skill);
        let skill_pathspec = Path::new(&skill_rel);
        let mut would_change = false;
        let mut files_changed = 0;
        let mut insertions = 0;
        let mut deletions = 0;
        let mut changed_paths = Vec::new();
        let mut truncated = false;
        if skill_exists && target_commit.is_some() && current_commit.is_some() {
            match gitops::diff_has_changes_from_ref(&self.ctx, &reference, skill_pathspec) {
                Ok(changed) => would_change = changed,
                Err(err) => risks.push(risk(
                    "error",
                    "GIT_ERROR",
                    format!("failed to compare rollback target '{}': {}", reference, err),
                )),
            }
            match gitops::diff_shortstat_from_ref(&self.ctx, &reference, skill_pathspec) {
                Ok(stat) => {
                    files_changed = stat.files_changed;
                    insertions = stat.insertions;
                    deletions = stat.deletions;
                }
                Err(err) => risks.push(risk(
                    "error",
                    "GIT_ERROR",
                    format!("failed to summarize rollback diff '{}': {}", reference, err),
                )),
            }
            match gitops::diff_changed_paths_from_ref(
                &self.ctx,
                &reference,
                skill_pathspec,
                ROLLBACK_PREVIEW_PATH_LIMIT,
            ) {
                Ok((paths, is_truncated)) => {
                    changed_paths = paths;
                    truncated = is_truncated;
                }
                Err(err) => risks.push(risk(
                    "error",
                    "GIT_ERROR",
                    format!(
                        "failed to list rollback diff paths '{}': {}",
                        reference, err
                    ),
                )),
            }
        }

        let (impacted_projections, projection_warnings) =
            rollback_impacted_projections(&self.ctx, &args.skill)?;
        for warning in projection_warnings {
            risks.push(risk(
                "warning",
                "REGISTRY_STATE_UNAVAILABLE",
                warning.clone(),
            ));
            warnings.push(warning);
        }
        let reproject_projection_ids = impacted_projections
            .iter()
            .filter(|projection| projection["requires_reproject"].as_bool() == Some(true))
            .filter_map(|projection| projection["instance_id"].as_str().map(ToString::to_string))
            .collect::<Vec<_>>();
        if !reproject_projection_ids.is_empty() {
            risks.push(risk(
                "warning",
                "STALE_LIVE_PROJECTIONS",
                format!(
                    "rollback does not update non-symlink live projections: {}",
                    reproject_projection_ids.join(", ")
                ),
            ));
        }

        Ok((
            json!({
                "preview": true,
                "dry_run": true,
                "operation": "skill.rollback",
                "safe_to_run": is_safe(&risks),
                "status": status_for(&risks, usize::from(target_commit.is_some())),
                "required_selectors": {
                    "skill": args.skill,
                    "reference": reference,
                },
                "skill": args.skill,
                "reference": reference,
                "target_commit": target_commit,
                "current_commit": current_commit,
                "resolved_ref": target_commit,
                "would_change": would_change,
                "diff": {
                    "files_changed": files_changed,
                    "insertions": insertions,
                    "deletions": deletions,
                    "changed_paths": changed_paths,
                    "truncated": truncated,
                },
                "impacted_projections": impacted_projections,
                "would_create_recovery_ref": would_change,
                "will_create_recovery_ref": would_change,
                "will_mutate": [],
                "rollback_would_mutate": ["skill_source", "git_history", "git_tags", "registry_ops"],
                "stale_projection_ids": reproject_projection_ids,
                "risks": risks,
            }),
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_skill_orphan_clean_plan(
        &self,
        args: &OrphanCleanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let snapshot = self.require_registry_snapshot()?;
        let projections = snapshot
            .projections
            .projections
            .iter()
            .filter(|projection| is_orphan_projection(projection))
            .collect::<Vec<_>>();
        let mut risks = Vec::new();
        let mut live_paths_to_delete = Vec::new();
        for projection in &projections {
            if args.delete_live_paths && Path::new(&projection.materialized_path).exists() {
                live_paths_to_delete.push(projection.materialized_path.clone());
            }
        }
        if !live_paths_to_delete.is_empty() {
            risks.push(risk(
                "warning",
                "LIVE_DELETE",
                format!(
                    "{} live orphan path(s) would be deleted",
                    live_paths_to_delete.len()
                ),
            ));
        }

        Ok((
            json!({
                "dry_run": true,
                "operation": "skill.orphan.clean",
                "safe_to_run": is_safe(&risks),
                "status": status_for(&risks, projections.len()),
                "delete_live_paths": args.delete_live_paths,
                "cleaned_projection_ids": projections.iter().map(|p| p.instance_id.clone()).collect::<Vec<_>>(),
                "live_paths_to_delete": live_paths_to_delete,
                "will_mutate": if args.delete_live_paths {
                    json!(["registry_state", "registry_ops", "live_target"])
                } else {
                    json!(["registry_state", "registry_ops"])
                },
                "risks": risks,
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_sync_push_plan(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        let operation_report = self
            .ctx
            .read_existing_registry_ops_report()
            .map_err(map_io)?;
        let mut risks = Vec::new();
        let remote_configured = gitops::remote_is_configured(&self.ctx).map_err(map_git)?;
        let tracking_ref = if remote_configured {
            gitops::remote_tracking_main_exists(&self.ctx).map_err(map_git)?
        } else {
            false
        };
        let mut ahead = None;
        let mut behind = None;
        if tracking_ref {
            let (a, b) = gitops::ahead_behind_main(&self.ctx).map_err(map_git)?;
            ahead = Some(a);
            behind = Some(b);
            if b > 0 {
                risks.push(risk(
                    "error",
                    "REMOTE_DIVERGED",
                    "local branch is behind origin/main",
                ));
            }
        }
        if !remote_configured {
            risks.push(risk(
                "error",
                "REMOTE_NOT_CONFIGURED",
                "remote origin not configured",
            ));
        }
        risks.push(risk(
            "warning",
            "REMOTE_STATUS_NOT_FETCHED",
            "dry-run does not fetch remote refs; result is based on local tracking refs",
        ));

        Ok((
            json!({
                "dry_run": true,
                "operation": "sync.push",
                "safe_to_run": is_safe(&risks),
                "status": status_for(&risks, usize::from(remote_configured)),
                "remote_configured": remote_configured,
                "tracking_ref": tracking_ref,
                "ahead": ahead,
                "behind": behind,
                "operation_backlog": operation_report.operation_counts.actionable_operations,
                "operation_counts": operation_report.operation_counts,
                "will_mutate": ["git_history", "remote", "registry_ops"],
                "risks": risks,
            }),
            Meta::default(),
        ))
    }
}

fn reconcile_visibility_unsupported_check(
    ctx: &crate::state::AppContext,
    agent: &str,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let adapter = match built_in_adapter_for_agent(ctx, agent) {
        Some(adapter) => Some(adapter),
        None => {
            let adapters = load_agent_adapters(ctx)?;
            adapters.adapter_for_agent(agent).cloned()
        }
    };
    let Some(adapter) = adapter else {
        return Ok(Some(visibility_unsupported_check(
            agent,
            format!("agent adapter '{}' is not registered", agent),
        )));
    };
    if reconcile_adapter_supports_visibility(&adapter) {
        return Ok(None);
    }
    Ok(Some(visibility_unsupported_check(
        agent,
        if adapter.fidelity.is_verified() {
            format!(
                "agent adapter '{}' does not expose visibility metadata",
                agent
            )
        } else {
            format!(
                "agent adapter '{}' has generic fidelity and does not expose verified visibility metadata",
                agent
            )
        },
    )))
}

fn reconcile_adapter_supports_visibility(adapter: &AgentAdapter) -> bool {
    adapter.has_verified_visibility_metadata()
}

fn visibility_unsupported_check(agent: &str, message: String) -> Value {
    json!({
        "id": "visibility_unsupported",
        "ok": false,
        "severity": "error",
        "message": message,
        "details": {"agent": agent},
        "next_action": format!("install or update the {agent} adapter visibility metadata")
    })
}
