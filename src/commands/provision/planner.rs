use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{ProvisionExportFormatArg, ProvisionTargetArg};
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};

use super::super::CommandFailure;
use super::super::helpers::{map_io, map_registry_state, shell_arg};
use super::super::skill_deps::skill_dependency_report;
use super::model::{
    PROVISION_PLAN_SCHEMA, ProvisionActiveView, ProvisionDependencyReadiness, ProvisionFilePlan,
    ProvisionPlan, ProvisionSecretRequirement,
};
use super::utils::{
    container_workspace_path, digest_file, digest_json, digest_str, normalize_clone_url,
    shell_safe_segment, target_skill_path, target_skill_path_relative, workspace_matches,
};

pub(super) fn build_provision_plan(
    ctx: &AppContext,
    target: ProvisionTargetArg,
    workspace: &Path,
    agent: &str,
) -> std::result::Result<ProvisionPlan, CommandFailure> {
    let target_kind = provision_target_name(target).to_string();
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
    let active_views = collect_active_views(
        ctx,
        snapshot.as_ref(),
        workspace,
        &container_workspace,
        agent,
        &mut findings,
    )?;
    let active_skills = active_views
        .iter()
        .flat_map(|view| view.skills.iter().cloned())
        .collect::<BTreeSet<_>>();
    let dependency_readiness =
        collect_dependency_readiness(ctx, &active_skills, agent, workspace, &mut findings)?;
    let mut secrets_required = collect_secret_requirements(ctx, &dependency_readiness);
    let (registry_source_display, registry_clone_url, registry_secrets) =
        registry_source(ctx, &mut findings);
    secrets_required.extend(registry_secrets);
    let files_to_write = devcontainer_file_plan(
        workspace,
        &container_workspace,
        agent,
        registry_clone_url.as_deref(),
        &active_views,
        &mut findings,
    );
    let active_view_digest = digest_json(&active_views)?;
    let dependency_readiness_digest = digest_json(&dependency_readiness)?;
    let files_digest = digest_json(&files_to_write)?;
    let registry_head = gitops::head(ctx).unwrap_or_else(|_| "working-tree".to_string());

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
            "apply_deferred": true,
            "secret_copy": false,
            "target_writes_in_plan": false,
            "approval_required_for_apply": true,
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
            "registry_head_reachable": gitops::head(ctx).is_ok(),
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
    agent: &str,
    findings: &mut Vec<Value>,
) -> std::result::Result<Vec<ProvisionActiveView>, CommandFailure> {
    let skillsets = load_skillsets(ctx)?;
    let target_path = target_skill_path(container_workspace, agent);
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
    let mut views = snapshot
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
        .map(|binding| {
            let skills = snapshot
                .rules
                .rules
                .iter()
                .filter(|rule| rule.binding_id == binding.binding_id)
                .map(|rule| rule.skill_id.clone())
                .collect::<BTreeSet<_>>();
            let target = targets.get(binding.default_target_id.as_str()).copied();
            ProvisionActiveView {
                agent: agent.to_string(),
                scope: "project".to_string(),
                path: target_path.clone(),
                binding_id: Some(binding.binding_id.clone()),
                source_target_id: Some(binding.default_target_id.clone()),
                source_target_path: target.map(|target| target.path.clone()),
                skillsets: skillsets_for_skills(&skillsets, &skills),
                skills: skills.into_iter().collect(),
            }
        })
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

fn collect_dependency_readiness(
    ctx: &AppContext,
    skills: &BTreeSet<String>,
    agent: &str,
    workspace: &Path,
    findings: &mut Vec<Value>,
) -> std::result::Result<Vec<ProvisionDependencyReadiness>, CommandFailure> {
    let mut readiness = Vec::new();
    for skill in skills {
        let report = skill_dependency_report(ctx, skill, Some(agent), Some(workspace))?;
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

fn collect_secret_requirements(
    ctx: &AppContext,
    dependencies: &[ProvisionDependencyReadiness],
) -> Vec<ProvisionSecretRequirement> {
    let mut by_name = BTreeMap::new();
    for dependency in dependencies {
        if let Ok(report) = skill_dependency_report(ctx, &dependency.skill, None, None) {
            for env in report.dependencies.env {
                by_name
                    .entry(env.name.clone())
                    .or_insert(ProvisionSecretRequirement {
                        name: env.name,
                        required: env.required,
                        present: env.present,
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
    agent: &str,
    registry_clone_url: Option<&str>,
    active_views: &[ProvisionActiveView],
    findings: &mut Vec<Value>,
) -> Vec<ProvisionFilePlan> {
    let setup =
        devcontainer_setup_script(container_workspace, agent, registry_clone_url, active_views);
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
    agent: &str,
    registry_clone_url: Option<&str>,
    active_views: &[ProvisionActiveView],
) -> String {
    let registry_block = match registry_clone_url {
        Some(url) => format!(
            "if [ ! -d \"$LOOM_REGISTRY_DIR/.git\" ]; then\n  git clone {} \"$LOOM_REGISTRY_DIR\"\nfi",
            shell_arg(url)
        ),
        None => "if [ ! -d \"$LOOM_REGISTRY_DIR/.git\" ]; then\n  echo \"LOOM_REGISTRY_DIR must contain a cloned Loom registry\" >&2\n  exit 1\nfi".to_string(),
    };
    let mut checks = String::new();
    for skill in active_views
        .iter()
        .flat_map(|view| view.skills.iter())
        .collect::<BTreeSet<_>>()
    {
        checks.push_str(&format!(
            "test -e \"$ACTIVE_VIEW/{}/SKILL.md\" || echo \"planned skill {} is not projected yet\" >&2\n",
            shell_safe_segment(skill),
            skill
        ));
    }
    if checks.is_empty() {
        checks.push_str("true\n");
    }

    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

WORKSPACE={}
LOOM_REGISTRY_DIR="${{LOOM_REGISTRY_DIR:-$HOME/.loom-registry}}"
ACTIVE_VIEW="$WORKSPACE/{}"

if ! command -v loom >/dev/null 2>&1; then
  echo "loom CLI is required before provisioning active skills" >&2
  exit 1
fi

{}
mkdir -p "$ACTIVE_VIEW"
loom --json --root "$LOOM_REGISTRY_DIR" workspace status >/dev/null
{}
"#,
        shell_arg(container_workspace),
        target_skill_path_relative(agent),
        registry_block,
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
            if normalized.had_userinfo {
                findings.push(json!({
                    "id": "registry_remote_credentials_redacted",
                    "severity": "warning",
                    "message": "registry remote URL contained userinfo and was redacted",
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
            (normalized.display, Some(normalized.clone_url), secrets)
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
