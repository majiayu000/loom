use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::{Value, json};

use super::McpRequirement;
use crate::commands::skill_deps::support::find_executable_on_path;

#[derive(Debug, Clone, Serialize)]
pub(super) struct McpCatalogEntry {
    pub(super) server: &'static str,
    pub(super) description: &'static str,
    pub(super) transport: &'static str,
    pub(super) source: &'static str,
    pub(super) required_tool: &'static str,
    pub(super) trust: &'static str,
    pub(super) permissions: &'static [&'static str],
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct McpResolvedSource {
    pub(super) server: String,
    pub(super) locator: String,
    pub(super) kind: String,
    pub(super) pinned: bool,
    pub(super) package: Option<String>,
    pub(super) version: Option<String>,
    pub(super) trust: String,
    pub(super) policy: String,
    pub(super) approval_required: Option<String>,
}

pub(super) fn resolve_source(req: &McpRequirement) -> McpResolvedSource {
    let catalog = catalog_entry(&req.server);
    let locator = req
        .source_locator
        .clone()
        .or_else(|| catalog.as_ref().map(|entry| entry.source.to_string()))
        .unwrap_or_else(|| req.server.clone());
    let (kind, pinned, package, version) = parse_mcp_locator(&locator);
    let catalog_match = catalog.is_some_and(|entry| entry.source == locator);
    let (policy, approval_required, trust) = if !pinned {
        (
            "blocked_unpinned".to_string(),
            Some("pin-mcp-source".to_string()),
            "unknown".to_string(),
        )
    } else if catalog_match {
        (
            "approval_required".to_string(),
            Some("install-third-party-mcp".to_string()),
            "catalog-pinned".to_string(),
        )
    } else {
        (
            "approval_required".to_string(),
            Some("install-unknown-mcp".to_string()),
            "unknown-pinned".to_string(),
        )
    };
    McpResolvedSource {
        server: req.server.clone(),
        locator,
        kind,
        pinned,
        package,
        version,
        trust,
        policy,
        approval_required,
    }
}

#[rustfmt::skip]
fn parse_mcp_locator(locator: &str) -> (String, bool, Option<String>, Option<String>) {
    if let Some(raw) = locator.strip_prefix("npm:") {
        let Some(index) = raw.rfind('@').filter(|index| *index > 0 && *index + 1 < raw.len()) else {
            return ("npm".to_string(), false, Some(raw.trim_end_matches('@').to_string()), None);
        };
        return ("npm".to_string(), true, Some(raw[..index].to_string()), Some(raw[index + 1..].to_string()));
    }
    if let Some(raw) = locator.strip_prefix("git:") {
        return ("git".to_string(), raw.split_once('#').is_some_and(|(_, rev)| looks_like_commit(rev)), Some(raw.to_string()), raw.split_once('#').map(|(_, rev)| rev.to_string()));
    }
    if let Some(raw) = locator.strip_prefix("local:") {
        return ("local".to_string(), raw.contains("@sha256:"), Some(raw.to_string()), raw.split_once("@sha256:").map(|(_, digest)| digest.to_string()));
    }
    if let Some(raw) = locator.strip_prefix("catalog:") {
        return ("catalog".to_string(), raw.split_once('@').is_some_and(|(_, rev)| !rev.is_empty()), Some(raw.to_string()), raw.split_once('@').map(|(_, rev)| rev.to_string()));
    }
    ("unknown".to_string(), false, Some(locator.to_string()), None)
}

fn looks_like_commit(value: &str) -> bool {
    value.len() >= 40 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub(super) fn tool_dependency(source: &McpResolvedSource) -> Vec<String> {
    match source.kind.as_str() {
        "npm" => vec!["tool:npx".to_string()],
        "local" => Vec::new(),
        "git" => vec!["tool:git".to_string()],
        _ => Vec::new(),
    }
}

pub(super) fn tool_availability(sources: &[McpResolvedSource]) -> Vec<Value> {
    let mut tools = BTreeSet::new();
    for source in sources {
        match source.kind.as_str() {
            "npm" => {
                tools.insert("npx");
            }
            "git" => {
                tools.insert("git");
            }
            _ => {}
        }
    }
    tools
        .into_iter()
        .map(|tool| json!({"tool": tool, "found": find_executable_on_path(tool).is_some()}))
        .collect()
}

pub(super) fn catalog_entries() -> Vec<McpCatalogEntry> {
    vec![
        McpCatalogEntry {
            server: "github",
            description: "GitHub MCP server for repository and issue workflows",
            transport: "stdio",
            source: "npm:@modelcontextprotocol/server-github@0.6.2",
            required_tool: "npx",
            trust: "third-party-pinned",
            permissions: &["repo:read", "issues:write"],
        },
        McpCatalogEntry {
            server: "filesystem",
            description: "Filesystem MCP server scoped by explicit paths",
            transport: "stdio",
            source: "npm:@modelcontextprotocol/server-filesystem@0.6.2",
            required_tool: "npx",
            trust: "third-party-pinned",
            permissions: &["files:read", "files:write"],
        },
    ]
}

pub(super) fn catalog_entry(server: &str) -> Option<McpCatalogEntry> {
    catalog_entries()
        .into_iter()
        .find(|entry| entry.server == server)
}
