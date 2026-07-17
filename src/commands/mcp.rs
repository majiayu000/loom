use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{
    McpCatalogCommand, McpCatalogSearchArgs, McpCatalogShowArgs, McpCommand, McpDoctorArgs,
    McpPlanArgs, McpRequirementCommand, McpRequirementListArgs,
};
use crate::envelope::Meta;
use crate::next_action_trace::observe_next_actions;
use crate::state::AppContext;
use crate::types::ErrorCode;

mod apply;
mod artifact;
mod config_status;
mod requirements;
mod source_policy;
mod utils;
use apply::cmd_mcp_apply;
use artifact::{MCP_PLAN_SCHEMA, skill_source_digest, write_durable_mcp_plan};
use config_status::{mcp_config_status, mcp_status_problem};
use requirements::collect_mcp_requirements;
use source_policy::{
    catalog_entries, catalog_entry, resolve_source, tool_availability, tool_dependency,
};
use utils::digest_str;

use super::codex_config::codex_config_path;
use super::helpers::map_io;
use super::skill_deps::skill_dependency_report;
use super::{App, CommandFailure};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct McpRequirement {
    server: String,
    required: bool,
    transport: String,
    source_locator: Option<String>,
    auth_env: Option<String>,
    permissions: Vec<String>,
    declared_in: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct McpEnvRequirement {
    name: String,
    present: bool,
    redacted: bool,
    declared_in: Vec<String>,
}

type McpRequirements = (Vec<McpRequirement>, Vec<McpEnvRequirement>, Vec<Value>);
type RequiredMcpPlanInputs = (
    Vec<McpRequirement>,
    Vec<source_policy::McpResolvedSource>,
    Vec<McpEnvRequirement>,
    Vec<Value>,
);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpAction {
    kind: String,
    server: Option<String>,
    safe_to_apply: bool,
    source: Option<String>,
    path: Option<String>,
    diff: Option<String>,
    diff_redacted: bool,
    depends_on: Vec<String>,
    approval_required: Option<String>,
    name: Option<String>,
    present: Option<bool>,
    details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpPlan {
    schema_version: String,
    plan_id: String,
    created_at: String,
    skill: String,
    agent: String,
    workspace: Option<String>,
    skill_source_digest: String,
    requirements: Vec<McpRequirement>,
    env: Vec<McpEnvRequirement>,
    resolved_sources: Vec<source_policy::McpResolvedSource>,
    tool_availability: Vec<Value>,
    actions: Vec<McpAction>,
    risk_summary: Value,
    approvals_required: Vec<String>,
    findings: Vec<Value>,
    writes_performed: bool,
}

impl App {
    pub fn cmd_mcp(
        &self,
        command: &McpCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            McpCommand::Requirement { command } => match command {
                McpRequirementCommand::List(args) => self.cmd_mcp_requirement_list(args),
            },
            McpCommand::Plan(args) => self.cmd_mcp_plan(args),
            McpCommand::Apply(args) => cmd_mcp_apply(&self.ctx, args),
            McpCommand::Doctor(args) => self.cmd_mcp_doctor(args),
            McpCommand::Catalog { command } => match command {
                McpCatalogCommand::Search(args) => self.cmd_mcp_catalog_search(args),
                McpCatalogCommand::Show(args) => self.cmd_mcp_catalog_show(args),
            },
        }
    }

    fn cmd_mcp_requirement_list(
        &self,
        args: &McpRequirementListArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let (requirements, env_requirements, findings) =
            collect_mcp_requirements(&self.ctx, &args.skill, args.agent.as_deref())?;
        Ok((
            json!({
                "skill": args.skill,
                "agent": args.agent,
                "requirements": requirements,
                "env": env_requirements,
                "findings": findings,
            }),
            Meta::default(),
        ))
    }

    fn cmd_mcp_plan(
        &self,
        args: &McpPlanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        reject_output_plan_inside_skill_source(&self.ctx, args)?;
        let plan = build_mcp_plan(&self.ctx, args)?;
        let durable_plan_path = write_durable_mcp_plan(&self.ctx, &plan)?;
        if let Some(output_plan) = &args.output_plan {
            let mut body = serde_json::to_string_pretty(&plan).map_err(map_io)?;
            body.push('\n');
            crate::fs_util::write_atomic(output_plan, &body).map_err(map_io)?;
        }

        let mut data = serde_json::to_value(&plan).map_err(map_io)?;
        if let Some(object) = data.as_object_mut() {
            object.insert(
                "durable_plan".to_string(),
                json!(durable_plan_path.display().to_string()),
            );
            object.insert("durable_plan_written".to_string(), json!(true));
            object.insert(
                "artifact_written".to_string(),
                json!(args.output_plan.is_some()),
            );
            object.insert(
                "output_plan".to_string(),
                json!(
                    args.output_plan
                        .as_ref()
                        .map(|path| path.display().to_string())
                ),
            );
        }
        Ok((data, Meta::default()))
    }

    fn cmd_mcp_doctor(
        &self,
        args: &McpDoctorArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let Some(skill) = &args.skill else {
            return Ok((
                json!({
                    "agent": args.agent,
                    "status": "manual_configuration_required",
                    "healthy": false,
                    "next_actions": observe_next_actions(
                        "mcp.doctor.manual",
                        ["run loom mcp doctor --skill <skill> --agent <agent>"],
                    ),
                    "writes_performed": false,
                }),
                Meta::default(),
            ));
        };
        let deps = skill_dependency_report(
            &self.ctx,
            skill,
            Some(&args.agent),
            args.workspace.as_deref(),
        )?;
        let (requirements, env_requirements, mut mcp_findings) =
            collect_mcp_requirements(&self.ctx, skill, Some(&args.agent))?;
        let mut next_actions = deps.next_actions.clone();
        let mut mcp_status = "ready";
        for req in &requirements {
            let resolved = resolve_source(req);
            let status = mcp_config_status(&args.agent, req, &resolved, &mut mcp_findings)?;
            mcp_status = combine_mcp_status(mcp_status, mcp_status_problem(&status));
        }
        for env_req in &env_requirements {
            if !env_req.present {
                mcp_status = combine_mcp_status(mcp_status, Some("blocked"));
                push_unique(&mut next_actions, format!("set {}", env_req.name));
                mcp_findings.push(json!({
                    "id": "mcp_env_missing",
                    "severity": "error",
                    "message": "required MCP environment variable is missing",
                    "details": { "env": env_req.name, "redacted": true },
                }));
            }
        }
        if mcp_status != "ready" {
            push_unique(
                &mut next_actions,
                format!("loom mcp plan --skill {} --agent {}", skill, args.agent),
            );
        }
        let status = combined_status(&deps.status, mcp_status);
        Ok((
            json!({
                "agent": args.agent,
                "skill": skill,
                "healthy": deps.ready && mcp_status == "ready",
                "status": status,
                "dependencies": deps,
                "mcp_requirements": requirements,
                "mcp_env": env_requirements,
                "mcp_findings": mcp_findings,
                "next_actions": observe_next_actions("mcp.doctor.report", next_actions),
                "writes_performed": false,
            }),
            Meta::default(),
        ))
    }

    fn cmd_mcp_catalog_search(
        &self,
        args: &McpCatalogSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let query = args.query.to_ascii_lowercase();
        let results = catalog_entries()
            .into_iter()
            .filter(|entry| {
                entry.server.contains(&query)
                    || entry.description.to_ascii_lowercase().contains(&query)
                    || entry.source.to_ascii_lowercase().contains(&query)
            })
            .collect::<Vec<_>>();
        Ok((
            json!({ "query": args.query, "results": results }),
            Meta::default(),
        ))
    }

    fn cmd_mcp_catalog_show(
        &self,
        args: &McpCatalogShowArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let entry = catalog_entry(&args.server).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("MCP catalog entry '{}' not found", args.server),
            )
        })?;
        Ok((json!({ "entry": entry }), Meta::default()))
    }
}

fn build_mcp_plan(
    ctx: &AppContext,
    args: &McpPlanArgs,
) -> std::result::Result<McpPlan, CommandFailure> {
    let (requirements, resolved_sources, env_requirements, mut findings) =
        current_required_mcp_plan_inputs(ctx, &args.skill, &args.agent)?;
    let mut actions = Vec::new();
    let mut approvals = BTreeSet::new();
    let mut risk_secret_names = BTreeSet::new();
    let mut external_package = false;

    for (req, resolved) in requirements.iter().zip(resolved_sources.iter()) {
        if let Some(approval) = &resolved.approval_required {
            approvals.insert(approval.clone());
        }
        if resolved.kind == "npm" || resolved.kind == "git" {
            external_package = true;
        }
        let existing = mcp_config_status(&args.agent, req, resolved, &mut findings)?;
        actions.push(McpAction {
            kind: "install_server".to_string(),
            server: Some(req.server.clone()),
            safe_to_apply: false,
            source: Some(resolved.locator.clone()),
            path: None,
            diff: None,
            diff_redacted: true,
            depends_on: tool_dependency(resolved),
            approval_required: resolved.approval_required.clone(),
            name: None,
            present: None,
            details: json!({
                "configured": existing,
                "policy": resolved.policy,
                "transport": req.transport,
            }),
        });
        if let Some(env_name) = &req.auth_env {
            risk_secret_names.insert(env_name.clone());
            actions.push(require_env_action(env_name));
        }
    }
    for env_req in &env_requirements {
        risk_secret_names.insert(env_req.name.clone());
        actions.push(require_env_action(&env_req.name));
    }

    let config_plan_actions = config_actions(&args.agent, &requirements, &mut findings)?;
    for action in &config_plan_actions {
        if let Some(approval) = &action.approval_required {
            approvals.insert(approval.clone());
        }
    }
    actions.extend(config_plan_actions);
    let network_access = !requirements.is_empty();
    let tool_availability = tool_availability(&resolved_sources);
    let missing_tools = tool_availability
        .iter()
        .filter(|tool| tool["found"] == json!(false))
        .map(|tool| tool["tool"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();
    for tool in &missing_tools {
        findings.push(json!({
            "id": "mcp_tool_missing",
            "severity": "warning",
            "message": "MCP server launcher tool is missing",
            "details": { "tool": tool },
        }));
    }

    Ok(McpPlan {
        schema_version: MCP_PLAN_SCHEMA.to_string(),
        plan_id: format!("mcpplan_{}", Uuid::new_v4().simple()),
        created_at: Utc::now().to_rfc3339(),
        skill: args.skill.clone(),
        agent: args.agent.clone(),
        workspace: args
            .workspace
            .as_ref()
            .map(|path| absolutize_path(path).map(|path| path.display().to_string()))
            .transpose()?,
        skill_source_digest: skill_source_digest(&ctx.skill_path(&args.skill))?,
        requirements,
        env: env_requirements,
        resolved_sources,
        tool_availability,
        actions,
        risk_summary: json!({
            "network_access": network_access,
            "secrets_required": risk_secret_names.into_iter().collect::<Vec<_>>(),
            "external_package": external_package,
            "config_write_guarded": true,
        }),
        approvals_required: approvals.into_iter().collect::<Vec<_>>(),
        findings,
        writes_performed: false,
    })
}

fn current_required_mcp_plan_inputs(
    ctx: &AppContext,
    skill: &str,
    agent: &str,
) -> std::result::Result<RequiredMcpPlanInputs, CommandFailure> {
    let (requirements, env_requirements, findings) =
        collect_mcp_requirements(ctx, skill, Some(agent))?;
    let requirements = required_mcp_requirements(&requirements);
    let skill_path = ctx.skill_path(skill);
    let resolved_sources = requirements
        .iter()
        .map(|req| resolved_source_for_plan(&skill_path, req))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok((requirements, resolved_sources, env_requirements, findings))
}

fn required_mcp_requirements(requirements: &[McpRequirement]) -> Vec<McpRequirement> {
    requirements
        .iter()
        .filter(|req| req.required)
        .cloned()
        .collect()
}

fn resolved_source_for_plan(
    skill_path: &Path,
    req: &McpRequirement,
) -> std::result::Result<source_policy::McpResolvedSource, CommandFailure> {
    let mut resolved = resolve_source(req);
    if resolved.kind == "local" {
        resolved.locator = absolutize_local_locator(skill_path, &resolved.locator)?;
        let (kind, pinned, package, version) = source_policy::parse_mcp_locator(&resolved.locator);
        resolved.kind = kind;
        resolved.pinned = pinned;
        resolved.package = package;
        resolved.version = version;
    }
    Ok(resolved)
}

fn absolutize_local_locator(
    skill_path: &Path,
    locator: &str,
) -> std::result::Result<String, CommandFailure> {
    let Some((raw_path, digest)) = locator
        .strip_prefix("local:")
        .and_then(|raw| raw.split_once("@sha256:"))
    else {
        return Ok(locator.to_string());
    };
    let path = PathBuf::from(raw_path);
    let path = if path.is_absolute() {
        path
    } else {
        skill_path.join(path)
    };
    let path = if path.exists() {
        fs::canonicalize(path).map_err(map_io)?
    } else {
        absolutize_path(&path)?
    };
    Ok(format!("local:{}@sha256:{digest}", path.display()))
}

fn reject_output_plan_inside_skill_source(
    ctx: &AppContext,
    args: &McpPlanArgs,
) -> std::result::Result<(), CommandFailure> {
    let Some(output_plan) = &args.output_plan else {
        return Ok(());
    };
    let skill_path = absolutize_path(&ctx.skill_path(&args.skill))?;
    let output_plan = absolutize_path(output_plan)?;
    if output_plan.starts_with(&skill_path) {
        let mut failure = CommandFailure::new(
            ErrorCode::ArgInvalid,
            "MCP plan output must not be written inside the skill source",
        );
        failure.details = json!({
            "skill": args.skill,
            "skill_path": skill_path.display().to_string(),
            "output_plan": output_plan.display().to_string(),
            "writes_performed": false,
        });
        return Err(failure);
    }
    Ok(())
}

fn absolutize_path(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir().map_err(map_io)?.join(path))
    }
}

fn combine_mcp_status(current: &'static str, next: Option<&'static str>) -> &'static str {
    match (current, next) {
        ("blocked", _) | (_, Some("blocked")) => "blocked",
        ("unknown", _) | (_, Some("unknown")) => "unknown",
        _ => "ready",
    }
}

fn combined_status(dependency_status: &str, mcp_status: &str) -> String {
    if dependency_status == "blocked" || mcp_status == "blocked" {
        "blocked".to_string()
    } else if dependency_status == "unknown" || mcp_status == "unknown" {
        "unknown".to_string()
    } else {
        "ready".to_string()
    }
}

fn push_unique(next_actions: &mut Vec<String>, action: String) {
    if !next_actions.iter().any(|existing| existing == &action) {
        next_actions.push(action);
    }
}

fn config_actions(
    agent: &str,
    requirements: &[McpRequirement],
    findings: &mut Vec<Value>,
) -> std::result::Result<Vec<McpAction>, CommandFailure> {
    if agent != "codex" {
        return Ok(requirements
            .iter()
            .map(|req| McpAction {
                kind: "manual_configuration_required".to_string(),
                server: Some(req.server.clone()),
                safe_to_apply: false,
                source: req.source_locator.clone(),
                path: None,
                diff: None,
                diff_redacted: true,
                depends_on: Vec::new(),
                approval_required: None,
                name: None,
                present: None,
                details: json!({"agent": agent, "transport": req.transport}),
            })
            .collect());
    }
    let path = codex_config_path()?;
    let preimage_digest = codex_config_preimage_digest(&path)?;
    let mut actions = Vec::new();
    for req in requirements {
        actions.push(McpAction {
            kind: "write_agent_config".to_string(),
            server: Some(req.server.clone()),
            safe_to_apply: false,
            source: req.source_locator.clone(),
            path: Some(path.display().to_string()),
            diff: Some(format!(
                "@@ redacted MCP config diff @@\n+[mcp_servers.{}]\n+command = \"<redacted-or-user-supplied>\"\n",
                req.server
            )),
            diff_redacted: true,
            depends_on: {
                let resolved = resolve_source(req);
                let mut deps = vec![format!("install_server:{}", req.server)];
                if let Some(env_name) = &req.auth_env {
                    deps.push(format!("require_env:{env_name}"));
                }
                deps.extend(tool_dependency(&resolved));
                deps.push("approval:write-agent-mcp-config".to_string());
                deps
            },
            approval_required: Some("write-agent-mcp-config".to_string()),
            name: None,
            present: None,
            details: json!({
                "agent": "codex",
                "config_exists": path.is_file(),
                "preimage_digest": preimage_digest,
            }),
        });
    }
    if requirements.is_empty() {
        findings.push(json!({
            "id": "mcp_no_requirements",
            "severity": "info",
            "message": "skill has no MCP requirements",
        }));
    }
    Ok(actions)
}

fn codex_config_preimage_digest(
    path: &Path,
) -> std::result::Result<Option<String>, CommandFailure> {
    match fs::read_to_string(path) {
        Ok(raw) => Ok(Some(digest_str(&raw))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(map_io(err)),
    }
}

fn require_env_action(name: &str) -> McpAction {
    McpAction {
        kind: "require_env".to_string(),
        server: None,
        safe_to_apply: false,
        source: None,
        path: None,
        diff: None,
        diff_redacted: true,
        depends_on: Vec::new(),
        approval_required: None,
        name: Some(name.to_string()),
        present: Some(env::var_os(name).is_some()),
        details: json!({"redacted": true}),
    }
}
