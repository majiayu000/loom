use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::{env, fs};

use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item};

use crate::commands::CommandFailure;
use crate::commands::helpers::{map_arg, validate_skill_name};
use crate::commands::skill_deps::support::{contains_word_token, yaml_dependency_values};
use crate::commands::skill_lint::frontmatter::parse_skill_frontmatter;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::source_policy::catalog_entry;
use super::{McpEnvRequirement, McpRequirement, McpRequirements};

pub(super) fn collect_mcp_requirements(
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
    read_mcp_agent_metadata(
        &skill_path,
        agent,
        &mut requirements,
        &mut env_requirements,
        &mut findings,
    );
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
                    if req.required {
                        env_requirements
                            .entry(env_name.to_string())
                            .or_default()
                            .insert(format!("mcp.{server}.auth"));
                    }
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
    findings: &mut Vec<Value>,
) {
    let Some(agent) = agent else {
        return;
    };
    for ext in ["yaml", "yml"] {
        let path = skill_path.join("agents").join(format!("{agent}.{ext}"));
        if !path.is_file() {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                findings.push(json!({
                    "id": "mcp_agent_metadata_read_failed",
                    "severity": "warning",
                    "message": err.to_string(),
                    "path": path,
                }));
                continue;
            }
        };
        let values = match yaml_dependency_values(&raw) {
            Ok(values) => values,
            Err(err) => {
                findings.push(json!({
                    "id": "mcp_agent_metadata_parse_failed",
                    "severity": "warning",
                    "message": err,
                    "path": path,
                }));
                continue;
            }
        };
        for (key, value) in values {
            if key == "requires_mcp" {
                add_csv_requirements(Some(&value), "agent metadata", requirements);
            } else if key == "requires_env" {
                add_csv_env(Some(&value), "agent metadata", env_requirements);
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
