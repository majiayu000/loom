use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::helpers::{
    agent_kind_as_str, map_arg, map_git, map_io, map_registry_state, projection_method_as_str,
    resolve_capture_projection, validate_skill_name,
};
use super::{App, CommandFailure};
use crate::cli::{AgentPreflightArgs, CaptureArgs, OrphanCleanArgs, ProjectArgs, RollbackArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::{RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths};

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
                    &binding.workspace_matcher.kind,
                    &binding.workspace_matcher.value,
                    &workspace,
                )
        }) {
            let method = args
                .skill
                .as_deref()
                .and_then(|skill| {
                    snapshot
                        .rules
                        .rules
                        .iter()
                        .find(|rule| {
                            rule.binding_id == binding.binding_id && rule.skill_id == skill
                        })
                        .map(|rule| rule.method.clone())
                })
                .unwrap_or_else(|| projection_method_as_str(args.method).to_string());
            let target = snapshot.target(&binding.default_target_id);
            if let Some(target) = target {
                push_target_risks(
                    &mut risks,
                    &snapshot,
                    &binding.binding_id,
                    &target.target_id,
                    &method,
                );
            } else {
                risks.push(risk(
                    "error",
                    "TARGET_NOT_FOUND",
                    format!(
                        "binding '{}' points at missing target '{}'",
                        binding.binding_id, binding.default_target_id
                    ),
                ));
            }
            matches.push(json!({
                "binding_id": binding.binding_id,
                "agent": binding.agent,
                "profile": binding.profile_id,
                "matcher": binding.workspace_matcher,
                "target_id": binding.default_target_id,
                "target": target,
                "method": method,
                "existing_projection": args.skill.as_deref().and_then(|skill| {
                    snapshot.projections.projections.iter().find(|projection| {
                        projection.skill_id == skill
                            && projection.binding_id.as_deref() == Some(binding.binding_id.as_str())
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
                "risks": risks,
                "next_commands": [format!(
                    "loom --json --root {} skill project {} --binding {} --method {}",
                    shell_arg(&self.ctx.root),
                    shell_arg(&args.skill),
                    shell_arg(&args.binding),
                    projection_method_as_str(args.method)
                )],
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_capture_plan(
        &self,
        args: &CaptureArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let snapshot = self.require_registry_snapshot()?;
        let mut risks = Vec::new();
        let projection = match resolve_capture_projection(&snapshot, args) {
            Ok(projection) => Some(projection),
            Err(err) => {
                risks.push(risk("error", err.code.as_str(), err.message));
                None
            }
        };

        if let Some(projection) = projection.as_ref() {
            if !self.ctx.skill_path(&projection.skill_id).exists() {
                risks.push(risk(
                    "error",
                    "SKILL_NOT_FOUND",
                    format!("skill '{}' not found", projection.skill_id),
                ));
            }
            let live_path = Path::new(&projection.materialized_path);
            if !live_path.exists() {
                risks.push(risk(
                    "error",
                    "LIVE_PATH_MISSING",
                    format!("projection path '{}' does not exist", live_path.display()),
                ));
            }
            if projection.method != "symlink" {
                risks.push(risk(
                    "warning",
                    "SOURCE_REPLACE",
                    format!(
                        "capture from '{}' would replace the registry source copy",
                        projection.instance_id
                    ),
                ));
            }
        }
        let target_paths = projection
            .iter()
            .map(|p| p.materialized_path.clone())
            .collect::<Vec<_>>();

        Ok((
            json!({
                "dry_run": true,
                "operation": "skill.capture",
                "safe_to_run": is_safe(&risks),
                "status": status_for(&risks, usize::from(projection.is_some())),
                "required_selectors": {
                    "skill": args.skill,
                    "binding_id": args.binding,
                    "instance_id": args.instance,
                },
                "projection": projection,
                "target_paths": target_paths,
                "will_mutate": ["skill_source", "registry_state", "registry_ops", "git_history"],
                "risks": risks,
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
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let mut risks = Vec::new();
        let remote_configured = gitops::remote_exists(&self.ctx);
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
                "pending_ops": pending_report.ops.len(),
                "will_mutate": ["git_history", "remote", "pending_queue"],
                "risks": risks,
            }),
            Meta {
                warnings: pending_report.warnings,
                ..Meta::default()
            },
        ))
    }
}

fn push_target_risks(
    risks: &mut Vec<Value>,
    snapshot: &RegistrySnapshot,
    binding_id: &str,
    target_id: &str,
    method: &str,
) {
    let Some(target) = snapshot.target(target_id) else {
        risks.push(risk(
            "error",
            "TARGET_NOT_FOUND",
            format!("target '{}' not found", target_id),
        ));
        return;
    };
    if target.ownership != "managed" {
        risks.push(risk(
            "error",
            "TARGET_NOT_MANAGED",
            format!(
                "target '{}' has ownership '{}' and cannot be written",
                target.target_id, target.ownership
            ),
        ));
    }
    if let Some(binding) = snapshot.binding(binding_id)
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
    match method {
        "symlink" if !target.capabilities.symlink => risks.push(risk(
            "error",
            "PROJECTION_METHOD_UNSUPPORTED",
            format!(
                "target '{}' does not support symlink projections",
                target.target_id
            ),
        )),
        "copy" | "materialize" if !target.capabilities.copy => risks.push(risk(
            "error",
            "PROJECTION_METHOD_UNSUPPORTED",
            format!(
                "target '{}' does not support copy/materialize projections",
                target.target_id
            ),
        )),
        _ => {}
    }
}

fn rollback_impacted_projections(
    ctx: &crate::state::AppContext,
    skill: &str,
) -> std::result::Result<(Vec<Value>, Vec<String>), CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let Some(snapshot) = paths.maybe_load_snapshot().map_err(map_registry_state)? else {
        return Ok((
            Vec::new(),
            vec![format!(
                "registry state not initialized under {}; impacted projections could not be determined",
                paths.registry_dir.display()
            )],
        ));
    };
    let projections = snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill)
        .map(|projection| {
            let requires_reproject = matches!(projection.method.as_str(), "copy" | "materialize");
            json!({
                "instance_id": projection.instance_id,
                "binding_id": projection.binding_id.as_deref(),
                "target_id": projection.target_id,
                "method": projection.method,
                "live_path": projection.materialized_path,
                "requires_reproject": requires_reproject,
            })
        })
        .collect();
    Ok((projections, Vec::new()))
}

fn build_preflight_next_commands(
    root: &Path,
    args: &AgentPreflightArgs,
    selectors: &Value,
) -> Vec<String> {
    let Some(skill) = args.skill.as_deref() else {
        return Vec::new();
    };
    let Some(binding_id) = selectors["binding_id"].as_str() else {
        return Vec::new();
    };
    let method = selectors["method"]
        .as_str()
        .unwrap_or_else(|| projection_method_as_str(args.method));
    vec![format!(
        "loom --json --root {} skill project {} --binding {} --method {}",
        shell_arg(root),
        shell_arg(skill),
        shell_arg(binding_id),
        method
    )]
}

fn target_paths(matches: &[Value]) -> Vec<String> {
    matches
        .iter()
        .filter_map(|entry| entry["target"]["path"].as_str().map(ToString::to_string))
        .collect()
}

fn normalize_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    absolute.canonicalize().unwrap_or(absolute)
}

fn workspace_matches(kind: &str, value: &str, workspace: &Path) -> bool {
    match kind {
        "path_prefix" => workspace.starts_with(normalize_path(Path::new(value))),
        "exact_path" => workspace == normalize_path(Path::new(value)),
        "name" => workspace.file_name().and_then(|name| name.to_str()) == Some(value),
        _ => false,
    }
}

fn is_orphan_projection(projection: &RegistryProjectionInstance) -> bool {
    projection.binding_id.is_none() && projection.health == "orphaned"
}

fn is_safe(risks: &[Value]) -> bool {
    !risks
        .iter()
        .any(|risk| risk["severity"].as_str() == Some("error"))
}

fn status_for(risks: &[Value], match_count: usize) -> &'static str {
    if !is_safe(risks) {
        return "blocked";
    }
    if match_count == 0 {
        return "no-op";
    }
    if risks
        .iter()
        .any(|risk| risk["severity"].as_str() == Some("warning"))
    {
        return "ready_with_warnings";
    }
    "ready"
}

fn risk(severity: &'static str, code: impl Into<String>, message: impl Into<String>) -> Value {
    json!({
        "severity": severity,
        "code": code.into(),
        "message": message.into(),
    })
}

fn shell_arg(value: impl AsRef<Path>) -> String {
    let raw = value.as_ref().display().to_string();
    if raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        raw
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    }
}
