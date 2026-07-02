use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;

use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item};
use uuid::Uuid;

use crate::cli::{
    McpCatalogCommand, McpCatalogSearchArgs, McpCatalogShowArgs, McpCommand, McpDoctorArgs,
    McpPlanArgs, McpRequirementCommand, McpRequirementListArgs,
};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

mod config_status;
mod source_policy;
use config_status::{mcp_config_status, mcp_status_problem};
use source_policy::{
    catalog_entries, catalog_entry, resolve_source, tool_availability, tool_dependency,
};

use super::codex_config::codex_config_path;
use super::helpers::{map_arg, validate_skill_name};
use super::skill_deps::skill_dependency_report;
use super::skill_deps::support::{contains_word_token, yaml_dependency_values};
use super::skill_lint::frontmatter::parse_skill_frontmatter;
use super::{App, CommandFailure};

#[derive(Debug, Clone, Serialize)]
struct McpRequirement {
    server: String,
    required: bool,
    transport: String,
    source_locator: Option<String>,
    auth_env: Option<String>,
    permissions: Vec<String>,
    declared_in: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct McpEnvRequirement {
    name: String,
    present: bool,
    redacted: bool,
    declared_in: Vec<String>,
}

type McpRequirements = (Vec<McpRequirement>, Vec<McpEnvRequirement>, Vec<Value>);

#[derive(Debug, Clone, Serialize)]
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
        let (requirements, env_requirements, mut findings) =
            collect_mcp_requirements(&self.ctx, &args.skill, Some(&args.agent))?;
        let mut actions = Vec::new();
        let mut resolved_sources = Vec::new();
        let mut approvals = BTreeSet::new();
        let mut risk_secret_names = BTreeSet::new();
        let mut external_package = false;

        for req in &requirements {
            let resolved = resolve_source(req);
            if let Some(approval) = &resolved.approval_required {
                approvals.insert(approval.clone());
            }
            if resolved.kind == "npm" || resolved.kind == "git" {
                external_package = true;
            }
            let existing = mcp_config_status(&args.agent, req, &resolved, &mut findings)?;
            actions.push(McpAction {
                kind: "install_server".to_string(),
                server: Some(req.server.clone()),
                safe_to_apply: false,
                source: Some(resolved.locator.clone()),
                path: None,
                diff: None,
                diff_redacted: true,
                depends_on: tool_dependency(&resolved),
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
            resolved_sources.push(resolved);
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

        Ok((
            json!({
                "plan_id": format!("mcpplan_{}", Uuid::new_v4().simple()),
                "created_at": Utc::now(),
                "skill": args.skill,
                "agent": args.agent,
                "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                "requirements": requirements,
                "resolved_sources": resolved_sources,
                "tool_availability": tool_availability,
                "actions": actions,
                "risk_summary": {
                    "network_access": !requirements.is_empty(),
                    "secrets_required": risk_secret_names.into_iter().collect::<Vec<_>>(),
                    "external_package": external_package,
                    "config_write_deferred": true,
                },
                "approvals_required": approvals.into_iter().collect::<Vec<_>>(),
                "findings": findings,
                "writes_performed": false,
            }),
            Meta::default(),
        ))
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
                    "next_actions": ["run loom mcp doctor --skill <skill> --agent <agent>"],
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
                "next_actions": next_actions,
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

fn collect_mcp_requirements(
    ctx: &AppContext,
    skill: &str,
    agent: Option<&str>,
) -> std::result::Result<McpRequirements, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let skill_path = ctx.skill_path(skill);
    if !skill_path.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    let mut requirements = BTreeMap::<String, McpRequirement>::new();
    let mut env_requirements = BTreeMap::<String, BTreeSet<String>>::new();
    let mut findings = Vec::new();
    read_mcp_manifest(
        &skill_path,
        &mut requirements,
        &mut env_requirements,
        &mut findings,
    );
    read_mcp_frontmatter(
        &skill_path,
        &mut requirements,
        &mut env_requirements,
        &mut findings,
    );
    read_mcp_agent_metadata(&skill_path, agent, &mut requirements, &mut env_requirements);
    Ok((
        requirements.into_values().collect(),
        env_requirements
            .into_iter()
            .map(|(name, sources)| McpEnvRequirement {
                present: env::var_os(&name).is_some(),
                redacted: true,
                name,
                declared_in: sources.into_iter().collect(),
            })
            .collect(),
        findings,
    ))
}

fn read_mcp_manifest(
    skill_path: &Path,
    requirements: &mut BTreeMap<String, McpRequirement>,
    env_requirements: &mut BTreeMap<String, BTreeSet<String>>,
    findings: &mut Vec<Value>,
) {
    let path = skill_path.join("loom.skill.toml");
    if !path.is_file() {
        return;
    }
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            findings.push(json!({"id":"mcp_manifest_read_failed","severity":"warning","message":err.to_string()}));
            return;
        }
    };
    let doc = match raw.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(err) => {
            findings.push(json!({"id":"mcp_manifest_toml_invalid","severity":"warning","message":err.to_string()}));
            return;
        }
    };
    add_array_requirements(doc.get("requires_mcp"), "loom.skill.toml", requirements);
    add_env_requirements(doc.get("requires_env"), "loom.skill.toml", env_requirements);
    if let Some(mcp) = doc.get("mcp").and_then(Item::as_table) {
        for (server, item) in mcp.iter() {
            let table = item.as_table_like();
            let req = requirement_entry(server, "loom.skill.toml", requirements);
            if let Some(table) = table {
                if let Some(required) = table.get("required").and_then(Item::as_bool) {
                    req.required = required;
                }
                if let Some(transport) = table.get("transport").and_then(Item::as_str) {
                    req.transport = transport.to_string();
                }
                if let Some(package) = table.get("package").and_then(Item::as_str) {
                    req.source_locator = Some(package.to_string());
                }
                if let Some(auth) = table.get("auth").and_then(Item::as_str)
                    && let Some(env_name) = auth.strip_prefix("env:")
                {
                    req.auth_env = Some(env_name.to_string());
                    env_requirements
                        .entry(env_name.to_string())
                        .or_default()
                        .insert(format!("mcp.{server}.auth"));
                }
                if let Some(permissions) = table.get("permissions").and_then(Item::as_array) {
                    req.permissions = permissions
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect();
                }
            }
        }
    }
}

fn read_mcp_frontmatter(
    skill_path: &Path,
    requirements: &mut BTreeMap<String, McpRequirement>,
    env_requirements: &mut BTreeMap<String, BTreeSet<String>>,
    findings: &mut Vec<Value>,
) {
    let entrypoint = skill_path.join("SKILL.md");
    if !entrypoint.is_file() {
        return;
    }
    let parsed = match parse_skill_frontmatter(&entrypoint) {
        Ok(parsed) => parsed.frontmatter,
        Err(err) => {
            findings.push(
                json!({"id":"mcp_frontmatter_parse_failed","severity":"warning","message":err}),
            );
            return;
        }
    };
    add_csv_requirements(
        parsed.metadata.get("loom.requires_mcp"),
        "SKILL.md metadata",
        requirements,
    );
    add_csv_env(
        parsed.metadata.get("loom.requires_env"),
        "SKILL.md metadata",
        env_requirements,
    );
    if let Some(compatibility) = parsed.compatibility {
        add_compatibility_suggestions(&compatibility_to_text(&compatibility), findings);
    }
}

fn read_mcp_agent_metadata(
    skill_path: &Path,
    agent: Option<&str>,
    requirements: &mut BTreeMap<String, McpRequirement>,
    env_requirements: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let Some(agent) = agent else {
        return;
    };
    for ext in ["yaml", "yml"] {
        let path = skill_path.join("agents").join(format!("{agent}.{ext}"));
        if let Ok(raw) = fs::read_to_string(path) {
            for (key, value) in yaml_dependency_values(&raw) {
                if key == "requires_mcp" {
                    add_csv_requirements(Some(&value), "agent metadata", requirements);
                } else if key == "requires_env" {
                    add_csv_env(Some(&value), "agent metadata", env_requirements);
                }
            }
        }
    }
}

fn add_array_requirements(
    item: Option<&Item>,
    source: &str,
    requirements: &mut BTreeMap<String, McpRequirement>,
) {
    if let Some(array) = item.and_then(Item::as_array) {
        for value in array.iter().filter_map(|value| value.as_str()) {
            add_requirement(value, source, requirements);
        }
    }
}

fn add_env_requirements(
    item: Option<&Item>,
    source: &str,
    env_requirements: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if let Some(array) = item.and_then(Item::as_array) {
        for value in array.iter().filter_map(|value| value.as_str()) {
            let value = value.trim();
            if !value.is_empty() {
                env_requirements
                    .entry(value.to_string())
                    .or_default()
                    .insert(source.to_string());
            }
        }
    }
}

fn add_csv_requirements(
    value: Option<&String>,
    source: &str,
    requirements: &mut BTreeMap<String, McpRequirement>,
) {
    if let Some(value) = value {
        for item in split_csv(value) {
            add_requirement(&item, source, requirements);
        }
    }
}

fn add_csv_env(
    value: Option<&String>,
    source: &str,
    env_requirements: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if let Some(value) = value {
        for item in split_csv(value) {
            env_requirements
                .entry(item)
                .or_default()
                .insert(source.to_string());
        }
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\''))
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn add_compatibility_suggestions(text: &str, findings: &mut Vec<Value>) {
    let lower = text.to_ascii_lowercase();
    for (needle, server) in [("github mcp", "github"), ("filesystem mcp", "filesystem")] {
        if lower.contains(needle) || (contains_word_token(&lower, server) && lower.contains("mcp"))
        {
            findings.push(json!({
                "id": "mcp_requirement_suggestion",
                "severity": "info",
                "message": "compatibility text suggests an MCP server; declare it in loom.skill.toml or metadata to make it required",
                "details": { "server": server, "source": "SKILL.md compatibility" },
            }));
        }
    }
}

fn compatibility_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
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

fn add_requirement(
    server: &str,
    source: &str,
    requirements: &mut BTreeMap<String, McpRequirement>,
) {
    let server = server.trim();
    if server.is_empty() {
        return;
    }
    requirement_entry(server, source, requirements);
}

fn requirement_entry<'a>(
    server: &str,
    source: &str,
    requirements: &'a mut BTreeMap<String, McpRequirement>,
) -> &'a mut McpRequirement {
    let entry = requirements
        .entry(server.to_string())
        .or_insert_with(|| McpRequirement {
            server: server.to_string(),
            required: true,
            transport: "stdio".to_string(),
            source_locator: catalog_entry(server).map(|entry| entry.source.to_string()),
            auth_env: None,
            permissions: Vec::new(),
            declared_in: Vec::new(),
        });
    if !entry.declared_in.iter().any(|existing| existing == source) {
        entry.declared_in.push(source.to_string());
    }
    entry
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
            details: json!({"agent": "codex", "config_exists": path.is_file()}),
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
