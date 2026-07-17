use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{ProvisionExportFormatArg, ProvisionTargetArg};
use crate::gitops;
use crate::next_action_trace::observe_next_actions;
use crate::state::AppContext;
use crate::state_model::{RegistryProjectionTarget, RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{map_io, map_registry_state, shell_arg};
use super::super::skill_deps::skill_dependency_report;
use super::super::skill_safety::evaluate_skill_safety_with_policy;
use super::model::{
    PROVISION_PLAN_SCHEMA, ProvisionActiveView, ProvisionDependencyReadiness, ProvisionFilePlan,
    ProvisionPlan, ProvisionSecretRequirement,
};
use super::utils::{
    container_workspace_path, digest_file, digest_json, digest_str, normalize_clone_url,
    normalize_existing_or_raw, path_to_slash, shell_safe_segment, target_skill_path,
    target_skill_path_relative, workspace_matches,
};

pub(super) fn build_provision_plan(
    ctx: &AppContext,
    target: ProvisionTargetArg,
    workspace: &Path,
    agent: &str,
) -> std::result::Result<ProvisionPlan, CommandFailure> {
    let target_kind = provision_target_name(target).to_string();
    let apply_deferred = target != ProvisionTargetArg::Devcontainer;
    let required_approvals = if apply_deferred {
        Vec::<&str>::new()
    } else {
        vec!["approval:provision-apply"]
    };
    let container_workspace = container_workspace_path(workspace);
    let mut findings = Vec::new();
    if target != ProvisionTargetArg::Devcontainer {
        findings.push(json!({
            "id": "provision_target_deferred",
            "severity": "warning",
            "message": "only devcontainer file previews are generated in this slice",
            "details": { "target": target_kind },
        }));
    }

    let paths = RegistryStatePaths::from_app_context(ctx);
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    let target_path_relative = target_skill_path_relative(ctx, workspace, agent)?;
    let active_views = collect_active_views(
        ctx,
        snapshot.as_ref(),
        workspace,
        &container_workspace,
        &target_path_relative,
        agent,
        &mut findings,
    )?;
    let active_skills = active_views
        .iter()
        .flat_map(|view| view.skills.iter().cloned())
        .collect::<BTreeSet<_>>();
    let dependency_readiness =
        collect_dependency_readiness(ctx, &active_skills, agent, workspace, &mut findings)?;
    collect_safety_policy_findings(
        ctx,
        snapshot.as_ref(),
        workspace,
        agent,
        &active_skills,
        &mut findings,
    )?;
    let mut secrets_required =
        collect_secret_requirements(ctx, &dependency_readiness, agent, workspace);
    let (registry_source_display, registry_clone_url, registry_secrets) =
        registry_source(ctx, &mut findings);
    secrets_required.extend(registry_secrets);
    let (registry_head, registry_head_reachable) = match gitops::head(ctx) {
        Ok(head) => (head, true),
        Err(_) => ("working-tree".to_string(), false),
    };
    let files_to_write = devcontainer_file_plan(
        workspace,
        &container_workspace,
        registry_clone_url.as_deref(),
        &registry_head,
        &active_views,
        &mut findings,
    );
    let active_view_digest = digest_json(&active_views)?;
    let dependency_readiness_digest = digest_json(&dependency_readiness)?;
    let files_digest = digest_json(&files_to_write)?;

    Ok(ProvisionPlan {
        schema_version: PROVISION_PLAN_SCHEMA.to_string(),
        plan_id: format!("provplan_{}", Uuid::new_v4().simple()),
        created_at: Utc::now(),
        target_kind,
        workspace: workspace.display().to_string(),
        container_workspace,
        agents: vec![agent.to_string()],
        registry_source_display,
        registry_clone_url,
        active_views,
        dependency_readiness,
        files_to_write,
        secrets_required,
        policy: json!({
            "mode": "plan_first",
            "apply_deferred": apply_deferred,
            "secret_copy": false,
            "target_writes_in_plan": false,
            "approval_required_for_apply": !apply_deferred,
            "required_approvals": required_approvals,
        }),
        loom_cli: json!({
            "required": true,
            "command": "loom",
            "version": env!("CARGO_PKG_VERSION"),
            "setup_check": "command -v loom",
        }),
        guards: json!({
            "root": ctx.root.display().to_string(),
            "registry_head": registry_head,
            "registry_head_reachable": registry_head_reachable,
            "active_view_digest": active_view_digest,
            "dependency_readiness_digest": dependency_readiness_digest,
            "files_digest": files_digest,
        }),
        findings,
    })
}

fn collect_active_views(
    ctx: &AppContext,
    snapshot: Option<&RegistrySnapshot>,
    workspace: &Path,
    container_workspace: &str,
    target_path_relative: &str,
    agent: &str,
    findings: &mut Vec<Value>,
) -> std::result::Result<Vec<ProvisionActiveView>, CommandFailure> {
    let skillsets = load_skillsets(ctx)?;
    let target_path = target_skill_path(container_workspace, target_path_relative);
    let Some(snapshot) = snapshot else {
        findings.push(json!({
            "id": "registry_state_missing",
            "severity": "warning",
            "message": "registry state is not initialized; active view will be empty",
            "details": {},
        }));
        return Ok(vec![empty_active_view(agent, target_path)]);
    };

    let targets = snapshot
        .targets
        .targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let matching_bindings = snapshot
        .bindings
        .bindings
        .iter()
        .filter(|binding| binding.active && binding.agent == agent)
        .filter(|binding| {
            workspace_matches(
                binding.workspace_matcher.kind.as_str(),
                &binding.workspace_matcher.value,
                workspace,
            )
        })
        .collect::<Vec<_>>();
    let mut grouped = BTreeMap::<(String, String, String, Option<String>), BTreeSet<String>>::new();
    for binding in matching_bindings {
        let mut saw_rule = false;
        for rule in snapshot
            .rules
            .rules
            .iter()
            .filter(|rule| rule.binding_id == binding.binding_id)
        {
            saw_rule = true;
            let target = targets.get(rule.target_id.as_str()).copied();
            if target.is_none() {
                findings.push(json!({
                    "id": "provision_rule_target_missing",
                    "severity": "warning",
                    "message": "binding rule references a target that is missing from registry state",
                    "details": {
                        "binding_id": binding.binding_id,
                        "target_id": rule.target_id,
                        "skill": rule.skill_id,
                    },
                }));
            }
            let view_path = active_view_path_for_target(
                target,
                workspace,
                container_workspace,
                target_path_relative,
                binding.default_target_id == rule.target_id,
                findings,
            );
            grouped
                .entry((
                    binding.binding_id.clone(),
                    rule.target_id.clone(),
                    view_path,
                    target.map(|target| target.path.clone()),
                ))
                .or_default()
                .insert(rule.skill_id.clone());
        }
        if !saw_rule {
            let target = targets.get(binding.default_target_id.as_str()).copied();
            grouped.entry((
                binding.binding_id.clone(),
                binding.default_target_id.clone(),
                target_path.clone(),
                target.map(|target| target.path.clone()),
            ));
        }
    }
    let mut views = grouped
        .into_iter()
        .map(
            |((binding_id, target_id, path, source_target_path), skills)| ProvisionActiveView {
                agent: agent.to_string(),
                scope: "project".to_string(),
                path,
                binding_id: Some(binding_id),
                source_target_id: Some(target_id),
                source_target_path,
                skillsets: skillsets_for_skills(&skillsets, &skills),
                skills: skills.into_iter().collect(),
            },
        )
        .collect::<Vec<_>>();

    if views.is_empty() {
        findings.push(json!({
            "id": "active_binding_missing",
            "severity": "warning",
            "message": "no active binding matched this workspace and agent",
            "details": {
                "agent": agent,
                "workspace": workspace.display().to_string(),
            },
        }));
        views.push(empty_active_view(agent, target_path));
    }

    Ok(views)
}

fn active_view_path_for_target(
    target: Option<&RegistryProjectionTarget>,
    workspace: &Path,
    container_workspace: &str,
    target_path_relative: &str,
    is_default_target: bool,
    findings: &mut Vec<Value>,
) -> String {
    let default_path = target_skill_path(container_workspace, target_path_relative);
    let Some(target) = target else {
        return default_path;
    };
    let target_path = normalize_existing_or_raw(Path::new(&target.path));
    let workspace = normalize_existing_or_raw(workspace);
    if target_path.starts_with(&workspace)
        && let Ok(relative) = target_path.strip_prefix(&workspace)
    {
        return target_skill_path(container_workspace, &path_to_slash(relative));
    }
    if is_default_target {
        return default_path;
    }
    findings.push(json!({
        "id": "provision_target_path_outside_workspace",
        "severity": "warning",
        "message": "non-default target path is outside the provisioned workspace and cannot be remapped into the devcontainer workspace",
        "details": {
            "target_id": target.target_id,
            "target_path": target.path,
            "workspace": workspace.display().to_string(),
        },
    }));
    target.path.clone()
}

fn collect_dependency_readiness(
    ctx: &AppContext,
    skills: &BTreeSet<String>,
    agent: &str,
    workspace: &Path,
    findings: &mut Vec<Value>,
) -> std::result::Result<Vec<ProvisionDependencyReadiness>, CommandFailure> {
    let mut readiness = Vec::new();
    for skill in skills {
        let report = match skill_dependency_report(ctx, skill, Some(agent), Some(workspace)) {
            Ok(report) => report,
            Err(err) if matches!(err.code, ErrorCode::SkillNotFound) => {
                findings.push(json!({
                    "id": "active_skill_missing",
                    "severity": "error",
                    "message": "active binding references a skill missing from the registry source",
                    "details": { "skill": skill },
                }));
                readiness.push(ProvisionDependencyReadiness {
                    skill: skill.clone(),
                    status: "missing".to_string(),
                    ready: false,
                    next_actions: observe_next_actions(
                        "provision.dependency.missing",
                        vec![format!(
                            "restore skill '{}' in the Loom registry before provisioning",
                            skill
                        )],
                    ),
                    findings: vec![json!({
                        "id": "active_skill_missing",
                        "severity": "error",
                        "message": "skill source is missing from the registry",
                        "suggested_action": "restore the skill source or remove the active binding rule",
                        "details": {},
                    })],
                });
                continue;
            }
            Err(err) => return Err(err),
        };
        for finding in &report.findings {
            findings.push(json!({
                "id": format!("dependency_{}", finding.id),
                "severity": finding.severity,
                "message": finding.message,
                "details": { "skill": skill, "source": finding.details },
            }));
        }
        readiness.push(ProvisionDependencyReadiness {
            skill: skill.clone(),
            status: report.status,
            ready: report.ready,
            next_actions: report.next_actions,
            findings: report
                .findings
                .into_iter()
                .map(|finding| {
                    json!({
                        "id": finding.id,
                        "severity": finding.severity,
                        "message": finding.message,
                        "suggested_action": finding.suggested_action,
                        "details": finding.details,
                    })
                })
                .collect(),
        });
    }
    Ok(readiness)
}

fn collect_safety_policy_findings(
    ctx: &AppContext,
    snapshot: Option<&RegistrySnapshot>,
    workspace: &Path,
    agent: &str,
    active_skills: &BTreeSet<String>,
    findings: &mut Vec<Value>,
) -> std::result::Result<(), CommandFailure> {
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let mut checked = BTreeSet::new();
    for binding in snapshot
        .bindings
        .bindings
        .iter()
        .filter(|binding| binding.active && binding.agent == agent)
        .filter(|binding| {
            workspace_matches(
                binding.workspace_matcher.kind.as_str(),
                &binding.workspace_matcher.value,
                workspace,
            )
        })
    {
        for rule in snapshot
            .rules
            .rules
            .iter()
            .filter(|rule| rule.binding_id == binding.binding_id)
        {
            if !active_skills.contains(&rule.skill_id)
                || !checked.insert((rule.skill_id.clone(), binding.policy_profile.clone()))
            {
                continue;
            }
            let evaluation = match evaluate_skill_safety_with_policy(
                ctx,
                &rule.skill_id,
                "provision",
                false,
                &binding.policy_profile,
            ) {
                Ok(evaluation) => evaluation,
                Err(err) if matches!(err.code, ErrorCode::SkillNotFound) => continue,
                Err(err) => return Err(err),
            };
            if !evaluation.report.activation_allowed {
                findings.push(json!({
                    "id": "skill_safety_policy_blocked",
                    "severity": "error",
                    "message": "active skill is blocked by safety or trust policy",
                    "details": {
                        "skill": rule.skill_id,
                        "binding_id": binding.binding_id,
                        "policy_profile": binding.policy_profile,
                        "decision": evaluation.report.decision,
                        "trust": {
                            "trust": evaluation.report.trust.trust,
                            "quarantined": evaluation.report.trust.quarantined,
                            "reason": evaluation.report.trust.reason,
                        },
                        "summary": evaluation.report.summary,
                    },
                }));
            }
        }
    }
    Ok(())
}

fn collect_secret_requirements(
    ctx: &AppContext,
    dependencies: &[ProvisionDependencyReadiness],
    agent: &str,
    workspace: &Path,
) -> Vec<ProvisionSecretRequirement> {
    let mut by_name = BTreeMap::new();
    for dependency in dependencies {
        if let Ok(report) =
            skill_dependency_report(ctx, &dependency.skill, Some(agent), Some(workspace))
        {
            for env in report.dependencies.env {
                by_name
                    .entry(env.name.clone())
                    .or_insert(ProvisionSecretRequirement {
                        name: env.name,
                        required: env.required,
                        present: false,
                        redacted: true,
                        source: env.source,
                    });
            }
        }
    }
    by_name.into_values().collect()
}

fn devcontainer_file_plan(
    workspace: &Path,
    container_workspace: &str,
    registry_clone_url: Option<&str>,
    registry_head: &str,
    active_views: &[ProvisionActiveView],
    findings: &mut Vec<Value>,
) -> Vec<ProvisionFilePlan> {
    let setup = devcontainer_setup_script(
        container_workspace,
        registry_clone_url,
        registry_head,
        active_views,
    );
    let devcontainer = devcontainer_json_preview();
    let mut files = Vec::new();
    for (path, kind, preview) in [
        (".devcontainer/loom-setup.sh", "shell", setup),
        (
            ".devcontainer/devcontainer.json",
            "devcontainer",
            devcontainer,
        ),
    ] {
        let absolute = workspace.join(path);
        let preimage_digest = digest_file(&absolute);
        let content_digest = digest_str(&preview);
        let safe_to_apply = preimage_digest
            .as_ref()
            .is_none_or(|digest| digest == &content_digest);
        if preimage_digest.is_some() && !safe_to_apply {
            findings.push(json!({
                "id": "provision_file_conflict",
                "severity": "warning",
                "message": "generated file path already exists with different content",
                "details": { "path": path },
            }));
        }
        files.push(ProvisionFilePlan {
            path: path.to_string(),
            kind: kind.to_string(),
            safe_to_apply,
            preimage_digest,
            content_digest,
            preview,
        });
    }
    files
}

fn devcontainer_setup_script(
    container_workspace: &str,
    registry_clone_url: Option<&str>,
    registry_head: &str,
    active_views: &[ProvisionActiveView],
) -> String {
    let registry_block = match registry_clone_url {
        Some(url) => format!(
            "if [ ! -d \"$LOOM_REGISTRY_DIR/.git\" ]; then\n  git clone {} \"$LOOM_REGISTRY_DIR\"\nfi",
            shell_arg(url)
        ),
        None => "if [ ! -d \"$LOOM_REGISTRY_DIR/.git\" ]; then\n  echo \"LOOM_REGISTRY_DIR must contain a cloned Loom registry\" >&2\n  exit 1\nfi".to_string(),
    };
    let registry_checkout = if registry_head == "working-tree" {
        "true".to_string()
    } else {
        format!(
            "REVIEWED_REGISTRY_HEAD={}\nif ! git -C \"$LOOM_REGISTRY_DIR\" rev-parse --verify --quiet \"${{REVIEWED_REGISTRY_HEAD}}^{{commit}}\" >/dev/null; then\n  git -C \"$LOOM_REGISTRY_DIR\" fetch --quiet origin \"$REVIEWED_REGISTRY_HEAD\"\nfi\ngit -C \"$LOOM_REGISTRY_DIR\" checkout --detach \"$REVIEWED_REGISTRY_HEAD\"",
            shell_arg(registry_head)
        )
    };
    let mut checks = String::new();
    for view in active_views {
        if view.skills.is_empty() {
            continue;
        }
        checks.push_str(&format!(
            "ACTIVE_VIEW={}\nmkdir -p \"$ACTIVE_VIEW\"\n",
            shell_arg(&view.path)
        ));
        for skill in &view.skills {
            checks.push_str(&format!(
                "if [ ! -e \"$ACTIVE_VIEW/{}/SKILL.md\" ]; then\n  echo \"planned skill {} is not projected at $ACTIVE_VIEW\" >&2\n  exit 1\nfi\n",
                shell_safe_segment(skill),
                skill
            ));
        }
    }
    if checks.is_empty() {
        checks.push_str("true\n");
    }

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

WORKSPACE={}
LOOM_REGISTRY_DIR="${{LOOM_REGISTRY_DIR:-$HOME/.loom-registry}}"

if ! command -v loom >/dev/null 2>&1; then
  echo "loom CLI is required before provisioning active skills" >&2
  exit 1
fi

{}
{}
loom --json --root "$LOOM_REGISTRY_DIR" workspace status >/dev/null
{}
"#,
        shell_arg(container_workspace),
        registry_block,
        registry_checkout,
        checks
    )
}

fn devcontainer_json_preview() -> String {
    serde_json::to_string_pretty(&json!({
        "postCreateCommand": "bash .devcontainer/loom-setup.sh"
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn registry_source(
    ctx: &AppContext,
    findings: &mut Vec<Value>,
) -> (String, Option<String>, Vec<ProvisionSecretRequirement>) {
    match gitops::remote_url(ctx) {
        Ok(Some(raw)) => {
            let normalized = normalize_clone_url(&raw);
            let mut secrets = Vec::new();
            if normalized.secret_redacted {
                findings.push(json!({
                    "id": "registry_remote_credentials_redacted",
                    "severity": "warning",
                    "message": "registry remote URL contained credential-bearing components and was redacted",
                    "details": { "secret": "git-credentials" },
                }));
                secrets.push(ProvisionSecretRequirement {
                    name: "GIT_CREDENTIALS".to_string(),
                    required: true,
                    present: false,
                    redacted: true,
                    source: "registry_remote".to_string(),
                });
            }
            if normalized.local_only {
                findings.push(json!({
                    "id": "registry_remote_local_only",
                    "severity": "warning",
                    "message": "registry origin remote is local-only and cannot be cloned from a remote devcontainer",
                    "details": {},
                }));
            }
            (normalized.display, normalized.clone_url, secrets)
        }
        Ok(None) => {
            findings.push(json!({
                "id": "registry_remote_missing",
                "severity": "warning",
                "message": "registry origin remote is not configured; use an export artifact or set a clone URL before remote provisioning",
                "details": {},
            }));
            ("local-only".to_string(), None, Vec::new())
        }
        Err(err) => {
            findings.push(json!({
                "id": "registry_remote_unreadable",
                "severity": "warning",
                "message": "registry origin remote could not be read",
                "details": { "error": err.to_string() },
            }));
            ("local-only".to_string(), None, Vec::new())
        }
    }
}

fn load_skillsets(ctx: &AppContext) -> std::result::Result<Value, CommandFailure> {
    let path = ctx.root.join("state/registry/skillsets.json");
    if !path.is_file() {
        return Ok(json!({ "schema_version": 1, "skillsets": [] }));
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    serde_json::from_str(&raw).map_err(map_io)
}

fn skillsets_for_skills(skillsets: &Value, skills: &BTreeSet<String>) -> Vec<String> {
    let mut matches = Vec::new();
    for skillset in skillsets["skillsets"].as_array().into_iter().flatten() {
        let id = skillset["id"].as_str().unwrap_or_default();
        if skillset["members"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|member| member["skill_id"].as_str())
            .any(|skill_id| skills.contains(skill_id))
        {
            matches.push(id.to_string());
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

fn empty_active_view(agent: &str, path: String) -> ProvisionActiveView {
    ProvisionActiveView {
        agent: agent.to_string(),
        scope: "project".to_string(),
        path,
        binding_id: None,
        source_target_id: None,
        source_target_path: None,
        skills: Vec::new(),
        skillsets: Vec::new(),
    }
}

pub(super) fn provision_target_name(target: ProvisionTargetArg) -> &'static str {
    match target {
        ProvisionTargetArg::Devcontainer => "devcontainer",
        ProvisionTargetArg::Codespaces => "codespaces",
        ProvisionTargetArg::Remote => "remote",
    }
}

pub(super) fn provision_export_format_name(format: ProvisionExportFormatArg) -> &'static str {
    match format {
        ProvisionExportFormatArg::Devcontainer => "devcontainer",
        ProvisionExportFormatArg::Shell => "shell",
        ProvisionExportFormatArg::Tar => "tar",
    }
}
