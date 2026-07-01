use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::SkillInspectArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::state_model::{
    RegistryBindingRule, RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths,
    RegistryWorkspaceBinding,
};
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_git, map_io, map_registry_state, validate_skill_name};
use super::provenance::{provenance_digest_status, provenance_record_status};
use super::skill_deps::skill_dependency_report;
use super::skill_safety::trust_metadata_for_skill;
use super::skill_verify::{
    drifted_paths_under, head_tree_oid_for_path, last_commit_for_path, last_saved_commit_for_skill,
};
use super::{App, CommandFailure, SkillLintMode, lint_skill_source, lint_skill_source_for_agent};

#[derive(Debug, Serialize)]
struct SourceStatus {
    path: String,
    exists: bool,
    entrypoint: Option<String>,
    entrypoint_exists: bool,
    working_tree_drift: bool,
    head_tree_oid: Option<String>,
    last_source_commit: Option<String>,
    drifted_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SpecStatus {
    portable: String,
    codex: String,
    claude: String,
    findings: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct ProvenanceStatus {
    source: Option<String>,
    pinned_ref: Option<String>,
    verified: Option<bool>,
    drift: Option<bool>,
}

#[derive(Debug, Serialize)]
struct RuntimeStatus {
    installed_in_registry: bool,
    active_rule_present: bool,
    projected_to_target: bool,
    materialized_path_exists: Option<bool>,
    visible_to_agent: String,
    enabled_by_agent_config: String,
    restart_required: String,
    target_id: Option<String>,
    binding_id: Option<String>,
    target_path: Option<String>,
    materialized_path: Option<String>,
    health: Option<String>,
    truth_level: String,
    findings: Vec<StatusFinding>,
}

#[derive(Debug, Serialize)]
struct StatusFinding {
    id: String,
    severity: String,
    message: String,
    next_action: Option<String>,
}

struct SourceGitStatus {
    head_tree_oid: Option<String>,
    last_source_commit: Option<String>,
    drifted_paths: Vec<String>,
}

struct Selector<'a> {
    agent: Option<&'a str>,
    workspace: Option<&'a Path>,
    profile: Option<&'a str>,
}

impl App {
    pub fn cmd_skill_inspect(
        &self,
        args: &SkillInspectArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let agent = args.agent.as_ref().map(|agent| agent.to_ascii_lowercase());
        let selector = Selector {
            agent: agent.as_deref(),
            workspace: args.workspace.as_deref(),
            profile: args.profile.as_deref(),
        };
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
        let skill_path = self.ctx.skill_path(&args.skill);
        let source_exists = skill_path.is_dir();
        let referenced = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot_references_skill(snapshot, &args.skill));

        if !source_exists && !referenced {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let source = build_source_status(&self.ctx, &args.skill, &skill_path, source_exists)?;
        let spec = build_spec_status(&self.ctx.root, &args.skill, &skill_path, source_exists);
        let provenance = build_provenance_status(&self.ctx, &args.skill, source_exists)?;
        let trust = trust_metadata_for_skill(&self.ctx, &args.skill)?;
        let dependencies = if source_exists {
            Some(skill_dependency_report(
                &self.ctx,
                &args.skill,
                selector.agent,
                selector.workspace,
            )?)
        } else {
            None
        };
        let runtime = build_runtime_status(
            &args.skill,
            &skill_path,
            source_exists,
            snapshot.as_ref(),
            selector,
        );
        let mut next_actions = build_next_actions(&args.skill, &spec, &runtime, source_exists);

        if spec.findings.iter().any(|finding| {
            finding["severity"].as_str() == Some("error")
                || finding["severity"].as_str() == Some("warning")
        }) {
            push_unique(
                &mut next_actions,
                format!("loom skill lint {} --portable", args.skill),
            );
        }
        push_unique(&mut next_actions, format!("loom skill eval {}", args.skill));
        push_unique(
            &mut next_actions,
            format!("loom skill policy {}", args.skill),
        );

        Ok((
            json!({
                "skill": args.skill,
                "source": source,
                "spec": spec,
                "provenance": provenance,
                "runtime": runtime,
                "dependencies": dependencies,
                "quality": {
                    "last_eval": Value::Null,
                    "trigger_precision": Value::Null,
                    "trigger_recall": Value::Null,
                    "baseline_delta": Value::Null,
                },
                "safety": {
                    "trust": trust.trust,
                    "policy": "unknown",
                    "scripts_present": Value::Null,
                    "network_requested": Value::Null,
                    "quarantined": trust.quarantined,
                    "reason": trust.reason,
                    "updated_at": trust.updated_at.map(|value| value.to_rfc3339()),
                },
                "next_actions": next_actions,
            }),
            Meta::default(),
        ))
    }
}

fn snapshot_references_skill(snapshot: &RegistrySnapshot, skill: &str) -> bool {
    snapshot
        .rules
        .rules
        .iter()
        .any(|rule| rule.skill_id == skill)
        || snapshot
            .projections
            .projections
            .iter()
            .any(|projection| projection.skill_id == skill)
}

fn build_source_status(
    ctx: &AppContext,
    skill: &str,
    skill_path: &Path,
    source_exists: bool,
) -> std::result::Result<SourceStatus, CommandFailure> {
    let lint = lint_skill_source(skill_path, skill, SkillLintMode::Compat);
    let git_status = source_git_status(ctx, skill, source_exists).map_err(map_git)?;
    Ok(SourceStatus {
        path: skill_path.display().to_string(),
        exists: source_exists,
        entrypoint: lint.entrypoint.file_name,
        entrypoint_exists: lint.entrypoint.path.is_some(),
        working_tree_drift: !git_status.drifted_paths.is_empty(),
        head_tree_oid: git_status.head_tree_oid,
        last_source_commit: git_status.last_source_commit,
        drifted_paths: git_status.drifted_paths,
    })
}

fn source_git_status(
    ctx: &AppContext,
    skill: &str,
    source_exists: bool,
) -> anyhow::Result<SourceGitStatus> {
    if !source_exists || !gitops::repo_is_initialized(ctx)? {
        return Ok(SourceGitStatus {
            head_tree_oid: None,
            last_source_commit: None,
            drifted_paths: Vec::new(),
        });
    }
    if !gitops::run_git_allow_failure(ctx, &["rev-parse", "--verify", "HEAD"])?
        .status
        .success()
    {
        return Ok(SourceGitStatus {
            head_tree_oid: None,
            last_source_commit: None,
            drifted_paths: Vec::new(),
        });
    }
    let skill_rel = format!("skills/{skill}");
    let head_tree_oid = head_tree_oid_for_path(ctx, &skill_rel)?;
    let last_source_commit = last_saved_commit_for_skill(ctx, skill)?
        .or_else(|| last_commit_for_path(ctx, &skill_rel).ok().flatten());
    let drifted_paths = drifted_paths_under(ctx, &skill_rel, last_source_commit.as_deref())?;
    Ok(SourceGitStatus {
        head_tree_oid,
        last_source_commit,
        drifted_paths,
    })
}

fn build_spec_status(
    root: &Path,
    skill: &str,
    skill_path: &Path,
    source_exists: bool,
) -> SpecStatus {
    if !source_exists {
        return SpecStatus {
            portable: "error".to_string(),
            codex: "not_checked".to_string(),
            claude: "not_checked".to_string(),
            findings: vec![json!({
                "id": "source_directory_missing",
                "severity": "error",
                "message": "skill source directory is missing",
                "suggested_action": "restore or import the skill source"
            })],
        };
    }

    let portable = lint_skill_source(skill_path, skill, SkillLintMode::Compat);
    let codex =
        lint_skill_source_for_agent(root, skill_path, skill, SkillLintMode::Compat, "codex");
    let claude =
        lint_skill_source_for_agent(root, skill_path, skill, SkillLintMode::Compat, "claude");
    let mut findings = Vec::new();
    findings.extend(portable.findings.iter().map(|finding| json!(finding)));
    findings.extend(codex.findings.iter().map(|finding| json!(finding)));
    findings.extend(claude.findings.iter().map(|finding| json!(finding)));
    SpecStatus {
        portable: portable.sections.portable_spec.status,
        codex: codex
            .sections
            .agent_compatibility
            .get("codex")
            .map(|section| section.status.clone())
            .unwrap_or_else(|| "pass".to_string()),
        claude: claude
            .sections
            .agent_compatibility
            .get("claude")
            .map(|section| section.status.clone())
            .unwrap_or_else(|| "pass".to_string()),
        findings,
    }
}

fn build_provenance_status(
    ctx: &AppContext,
    skill: &str,
    source_exists: bool,
) -> std::result::Result<ProvenanceStatus, CommandFailure> {
    let record = provenance_record_status(ctx, skill).map_err(map_io)?;
    let digest = if source_exists {
        provenance_digest_status(ctx, skill)?
    } else {
        None
    };
    let pinned_ref = record.as_ref().and_then(|record| {
        record
            .source
            .requested_ref
            .clone()
            .or_else(|| record.source.resolved_commit.clone())
    });
    Ok(ProvenanceStatus {
        source: record.as_ref().map(|record| record.source.locator.clone()),
        pinned_ref,
        verified: digest.as_ref().map(|status| status.matches),
        drift: digest.as_ref().map(|status| !status.matches),
    })
}

fn build_runtime_status(
    skill: &str,
    skill_path: &Path,
    source_exists: bool,
    snapshot: Option<&RegistrySnapshot>,
    selector: Selector<'_>,
) -> BTreeMap<String, RuntimeStatus> {
    let agents = runtime_agents(skill, snapshot, selector.agent);
    agents
        .into_iter()
        .map(|agent| {
            let status = classify_agent_runtime(
                skill,
                skill_path,
                &agent,
                source_exists,
                snapshot,
                &selector,
            );
            (agent, status)
        })
        .collect()
}

fn runtime_agents(
    skill: &str,
    snapshot: Option<&RegistrySnapshot>,
    selected_agent: Option<&str>,
) -> BTreeSet<String> {
    if let Some(agent) = selected_agent {
        return BTreeSet::from([agent.to_string()]);
    }
    let mut agents = BTreeSet::new();
    if let Some(snapshot) = snapshot {
        for target in &snapshot.targets.targets {
            agents.insert(target.agent.clone());
        }
        for binding in &snapshot.bindings.bindings {
            agents.insert(binding.agent.clone());
        }
        for rule in &snapshot.rules.rules {
            if rule.skill_id == skill {
                if let Some(target) = snapshot.target(&rule.target_id) {
                    agents.insert(target.agent.clone());
                } else if let Some(binding) = snapshot.binding(&rule.binding_id) {
                    agents.insert(binding.agent.clone());
                }
            }
        }
        for projection in &snapshot.projections.projections {
            if projection.skill_id == skill
                && let Some(target) = snapshot.target(&projection.target_id)
            {
                agents.insert(target.agent.clone());
            }
        }
    }
    if agents.is_empty() {
        agents.insert("claude".to_string());
        agents.insert("codex".to_string());
    }
    agents
}

fn classify_agent_runtime(
    skill: &str,
    skill_path: &Path,
    agent: &str,
    source_exists: bool,
    snapshot: Option<&RegistrySnapshot>,
    selector: &Selector<'_>,
) -> RuntimeStatus {
    let Some(snapshot) = snapshot else {
        return RuntimeStatus {
            installed_in_registry: source_exists,
            active_rule_present: false,
            projected_to_target: false,
            materialized_path_exists: None,
            visible_to_agent: "not_checked".to_string(),
            enabled_by_agent_config: "not_checked".to_string(),
            restart_required: "not_checked".to_string(),
            target_id: None,
            binding_id: None,
            target_path: None,
            materialized_path: None,
            health: None,
            truth_level: "source_only".to_string(),
            findings: vec![finding(
                "registry_state_missing",
                "warning",
                "registry state is not initialized; runtime projection state is not checked",
                None,
            )],
        };
    };

    let rules = matching_rules(snapshot, skill, agent, selector);
    let projections = matching_projections(snapshot, skill, agent, selector);
    let primary_rule = rules.first();
    let primary_projection = projections.first();
    let primary_target = primary_projection
        .and_then(|projection| snapshot.target(&projection.target_id))
        .or_else(|| primary_rule.and_then(|rule| snapshot.target(&rule.target_id)));
    let primary_binding = primary_projection
        .and_then(|projection| projection.binding_id.as_deref())
        .and_then(|binding_id| snapshot.binding(binding_id))
        .or_else(|| primary_rule.and_then(|rule| snapshot.binding(&rule.binding_id)));
    let materialized_path =
        primary_projection.map(|projection| projection.materialized_path.clone());
    let mut findings = runtime_findings(
        source_exists,
        skill_path,
        snapshot,
        primary_rule,
        primary_projection,
        &projections,
    );

    if !rules.is_empty() && projections.is_empty() {
        let action = primary_rule
            .map(|rule| format!("loom skill project {skill} --binding {}", rule.binding_id));
        findings.push(finding(
            "projection_missing",
            "warning",
            "active registry rule exists but no projection instance was found",
            action,
        ));
    }

    let materialized_path_exists = materialized_path
        .as_deref()
        .map(|path| Path::new(path).exists());
    let projected = !projections.is_empty();
    let visibility = if projected || !rules.is_empty() {
        "unknown"
    } else {
        "not_checked"
    };

    RuntimeStatus {
        installed_in_registry: source_exists,
        active_rule_present: !rules.is_empty(),
        projected_to_target: projected,
        materialized_path_exists,
        visible_to_agent: visibility.to_string(),
        enabled_by_agent_config: visibility.to_string(),
        restart_required: visibility.to_string(),
        target_id: primary_target.map(|target| target.target_id.clone()),
        binding_id: primary_binding.map(|binding| binding.binding_id.clone()),
        target_path: primary_target.map(|target| target.path.clone()),
        materialized_path,
        health: primary_projection.map(|projection| projection.health.clone()),
        truth_level: "registry_projection".to_string(),
        findings,
    }
}

fn matching_rules<'a>(
    snapshot: &'a RegistrySnapshot,
    skill: &str,
    agent: &str,
    selector: &Selector<'_>,
) -> Vec<&'a RegistryBindingRule> {
    snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| {
            rule.skill_id == skill
                && rule_agent(snapshot, rule).as_deref() == Some(agent)
                && binding_matches(snapshot.binding(&rule.binding_id), selector)
        })
        .collect()
}

fn matching_projections<'a>(
    snapshot: &'a RegistrySnapshot,
    skill: &str,
    agent: &str,
    selector: &Selector<'_>,
) -> Vec<&'a RegistryProjectionInstance> {
    snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| {
            let projection_agent = snapshot
                .target(&projection.target_id)
                .map(|target| target.agent.as_str())
                .or_else(|| {
                    projection
                        .binding_id
                        .as_deref()
                        .and_then(|binding_id| snapshot.binding(binding_id))
                        .map(|binding| binding.agent.as_str())
                });
            projection.skill_id == skill
                && projection_agent.is_none_or(|projection_agent| projection_agent == agent)
                && binding_matches(
                    projection
                        .binding_id
                        .as_deref()
                        .and_then(|binding_id| snapshot.binding(binding_id)),
                    selector,
                )
        })
        .collect()
}

fn rule_agent(snapshot: &RegistrySnapshot, rule: &RegistryBindingRule) -> Option<String> {
    snapshot
        .target(&rule.target_id)
        .map(|target| target.agent.clone())
        .or_else(|| {
            snapshot
                .binding(&rule.binding_id)
                .map(|binding| binding.agent.clone())
        })
}

fn binding_matches(binding: Option<&RegistryWorkspaceBinding>, selector: &Selector<'_>) -> bool {
    let Some(binding) = binding else {
        return selector.workspace.is_none() && selector.profile.is_none();
    };
    if !binding.active {
        return false;
    }
    if let Some(profile) = selector.profile
        && binding.profile_id != profile
    {
        return false;
    }
    let Some(workspace) = selector.workspace else {
        return true;
    };
    let workspace = workspace.to_string_lossy();
    let matcher = &binding.workspace_matcher;
    match matcher.kind.as_str() {
        "path_prefix" => Path::new(workspace.as_ref()).starts_with(Path::new(&matcher.value)),
        "exact_path" => workspace == matcher.value,
        "name" => {
            Path::new(workspace.as_ref())
                .file_name()
                .and_then(|name| name.to_str())
                == Some(matcher.value.as_str())
        }
        _ => false,
    }
}

fn runtime_findings(
    source_exists: bool,
    skill_path: &Path,
    snapshot: &RegistrySnapshot,
    primary_rule: Option<&&RegistryBindingRule>,
    _primary_projection: Option<&&RegistryProjectionInstance>,
    projections: &[&RegistryProjectionInstance],
) -> Vec<StatusFinding> {
    let mut findings = Vec::new();
    if !source_exists {
        findings.push(finding(
            "source_missing",
            "error",
            "registry references this skill but the source directory is missing",
            Some("restore or import the skill source before projection".to_string()),
        ));
    }
    if let Some(rule) = primary_rule {
        match (
            snapshot.binding(&rule.binding_id),
            snapshot.target(&rule.target_id),
        ) {
            (None, _) => findings.push(finding(
                "binding_missing",
                "error",
                "registry rule references a missing binding",
                Some("inspect and repair state/registry/rules.json".to_string()),
            )),
            (Some(binding), Some(target)) if binding.agent != target.agent => {
                findings.push(finding(
                    "target_agent_mismatch",
                    "error",
                    "rule binding and target refer to different agents",
                    Some("update the binding target or recreate the projection".to_string()),
                ));
            }
            (_, None) => findings.push(finding(
                "target_missing",
                "error",
                "registry rule references a missing target",
                Some("loom target list".to_string()),
            )),
            _ => {}
        }
    }
    for projection in projections {
        if snapshot.target(&projection.target_id).is_none() {
            findings.push(finding(
                "target_missing",
                "error",
                "projection references a missing target",
                Some("loom target list".to_string()),
            ));
        }
        if let Some(binding_id) = projection.binding_id.as_deref()
            && snapshot.binding(binding_id).is_none()
        {
            findings.push(finding(
                "binding_missing",
                "error",
                "projection references a missing binding",
                Some("loom workspace binding list".to_string()),
            ));
        }
        if projection.health != "healthy" {
            findings.push(finding(
                "projection_health",
                "warning",
                format!("projection health is {}", projection.health),
                Some(format!("loom skill diagnose {}", projection.skill_id)),
            ));
        }
        inspect_materialized_path(projection, skill_path, source_exists, &mut findings);
    }
    findings
}

fn inspect_materialized_path(
    projection: &RegistryProjectionInstance,
    skill_path: &Path,
    source_exists: bool,
    findings: &mut Vec<StatusFinding>,
) {
    let path = PathBuf::from(&projection.materialized_path);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                match fs::canonicalize(&path) {
                    Ok(actual) => {
                        if source_exists
                            && let Ok(expected) = fs::canonicalize(skill_path)
                            && actual != expected
                        {
                            findings.push(finding(
                                "projection_source_mismatch",
                                "error",
                                "projection symlink points to a different source path",
                                Some(format!(
                                    "loom skill project {} --binding <binding-id>",
                                    projection.skill_id
                                )),
                            ));
                        }
                    }
                    Err(_) => findings.push(finding(
                        "broken_symlink",
                        "error",
                        "projection path is a symlink whose target is missing",
                        Some(format!(
                            "loom skill project {} --binding <binding-id>",
                            projection.skill_id
                        )),
                    )),
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => findings.push(finding(
            "materialized_path_missing",
            "error",
            "projection materialized path is missing",
            Some(format!(
                "loom skill project {} --binding <binding-id>",
                projection.skill_id
            )),
        )),
        Err(err) => findings.push(finding(
            "materialized_path_unreadable",
            "error",
            format!("projection materialized path could not be inspected: {err}"),
            Some(format!(
                "inspect filesystem permissions for {}",
                path.display()
            )),
        )),
    }
}

fn finding(
    id: impl Into<String>,
    severity: impl Into<String>,
    message: impl Into<String>,
    next_action: Option<String>,
) -> StatusFinding {
    StatusFinding {
        id: id.into(),
        severity: severity.into(),
        message: message.into(),
        next_action,
    }
}

fn build_next_actions(
    skill: &str,
    spec: &SpecStatus,
    runtime: &BTreeMap<String, RuntimeStatus>,
    source_exists: bool,
) -> Vec<String> {
    let mut actions = Vec::new();
    if !source_exists {
        push_unique(
            &mut actions,
            format!("restore or import skill '{skill}' before using it"),
        );
        return actions;
    }
    if spec.portable == "error" || spec.codex == "error" || spec.claude == "error" {
        push_unique(&mut actions, format!("loom skill lint {skill} --portable"));
    }
    for status in runtime.values() {
        if status.active_rule_present
            && !status.projected_to_target
            && let Some(binding_id) = status.binding_id.as_deref()
        {
            push_unique(
                &mut actions,
                format!("loom skill project {skill} --binding {binding_id}"),
            );
        }
        if !status.active_rule_present && !status.projected_to_target {
            push_unique(
                &mut actions,
                format!(
                    "loom workspace binding list && loom skill project {skill} --binding <binding-id>"
                ),
            );
        }
        if status.visible_to_agent == "unknown" {
            push_unique(&mut actions, format!("loom skill diagnose {skill}"));
        }
        for finding in &status.findings {
            if let Some(action) = finding.next_action.as_ref() {
                push_unique(&mut actions, action.clone());
            }
        }
    }
    actions
}

fn push_unique(actions: &mut Vec<String>, action: String) {
    if !actions.iter().any(|existing| existing == &action) {
        actions.push(action);
    }
}
