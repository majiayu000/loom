use std::collections::BTreeSet;
use std::fs;

use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item, TableLike};

use super::McpRequirement;
use super::source_policy::McpResolvedSource;
use crate::commands::CommandFailure;
use crate::commands::codex_config::codex_config_path;

pub(super) fn mcp_config_status(
    agent: &str,
    req: &McpRequirement,
    resolved: &McpResolvedSource,
    findings: &mut Vec<Value>,
) -> std::result::Result<Value, CommandFailure> {
    if agent != "codex" {
        return Ok(json!({"status": "unknown", "present": "unknown", "compatible": "unknown"}));
    }
    let path = codex_config_path()?;
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            push_missing(req, findings);
            return Ok(json!({"status": "missing", "present": false, "compatible": false}));
        }
        Err(err) => {
            findings.push(json!({
                "id": "mcp_config_read_failed",
                "severity": "warning",
                "message": "Codex MCP config could not be read",
                "details": { "error": err.to_string(), "redacted": true },
            }));
            return Ok(json!({"status": "unknown", "present": "unknown", "compatible": "unknown"}));
        }
    };
    let doc = match raw.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(err) => {
            findings.push(json!({
                "id": "mcp_config_parse_failed",
                "severity": "warning",
                "message": "Codex MCP config could not be parsed",
                "details": { "error": err.to_string(), "redacted": true },
            }));
            return Ok(json!({"status": "unknown", "present": "unknown", "compatible": "unknown"}));
        }
    };
    let Some(table) = doc
        .get("mcp_servers")
        .and_then(Item::as_table)
        .and_then(|servers| servers.get(&req.server))
        .and_then(Item::as_table_like)
    else {
        push_missing(req, findings);
        return Ok(json!({"status": "missing", "present": false, "compatible": false}));
    };
    let mismatches = config_mismatches(table, req, resolved);
    if !mismatches.is_empty() {
        findings.push(json!({
            "id": "mcp_config_mismatch",
            "severity": "error",
            "message": "configured MCP server does not match the declared requirement",
            "details": { "mcp": req.server, "agent": "codex", "mismatches": mismatches },
        }));
        return Ok(
            json!({"status": "mismatch", "present": true, "compatible": false, "mismatches": mismatches}),
        );
    }
    Ok(json!({"status": "compatible", "present": true, "compatible": true}))
}

pub(super) fn mcp_status_problem(status: &Value) -> Option<&'static str> {
    match status.get("status").and_then(Value::as_str) {
        Some("missing" | "mismatch") => Some("blocked"),
        Some("unknown") => Some("unknown"),
        _ => None,
    }
}

fn push_missing(req: &McpRequirement, findings: &mut Vec<Value>) {
    findings.push(json!({
        "id": "mcp_missing",
        "severity": "error",
        "message": "required MCP server is not configured",
        "details": { "mcp": req.server, "agent": "codex" },
    }));
}

fn config_mismatches(
    table: &dyn TableLike,
    req: &McpRequirement,
    resolved: &McpResolvedSource,
) -> Vec<String> {
    let mut mismatches = BTreeSet::new();
    if let Some(transport) = table.get("transport").and_then(Item::as_str)
        && transport != req.transport
    {
        mismatches.insert("transport".to_string());
    }
    if req.transport == "stdio" && table.get("url").is_some() {
        mismatches.insert("transport".to_string());
    }
    if let Some(env_name) = &req.auth_env
        && !config_item_has_key(table.get("env"), env_name)
    {
        mismatches.insert(format!("env:{env_name}"));
    }
    let config_text = mcp_config_command_text(table);
    if resolved.kind == "npm" {
        if let Some(package) = &resolved.package
            && !config_text.contains(package)
        {
            mismatches.insert("source_package".to_string());
        }
        if let (Some(package), Some(version)) = (&resolved.package, &resolved.version)
            && !config_text.contains(&format!("{package}@{version}"))
        {
            mismatches.insert("source_version".to_string());
        }
    }
    mismatches.into_iter().collect()
}

fn mcp_config_command_text(table: &dyn TableLike) -> String {
    let mut parts = Vec::new();
    if let Some(command) = table.get("command").and_then(Item::as_str) {
        parts.push(command.to_string());
    }
    if let Some(args) = table.get("args").and_then(Item::as_array) {
        parts.extend(
            args.iter()
                .filter_map(|arg| arg.as_str().map(str::to_string)),
        );
    }
    parts.join(" ")
}

fn config_item_has_key(item: Option<&Item>, key: &str) -> bool {
    item.and_then(Item::as_table_like)
        .is_some_and(|table| table.contains_key(key))
}
