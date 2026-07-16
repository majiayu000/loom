use std::env;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::state::AppContext;

use super::support::{codex_mcp_configured, find_executable_on_path};
use super::{
    DependencyFinding, EnvDependency, McpDependency, NetworkDependency, ToolDependency, dep_finding,
};

const VERSION_TIMEOUT: Duration = Duration::from_millis(250);

pub(super) fn probe_tool(
    tool: &str,
    source: &str,
    next_actions: &mut Vec<String>,
    findings: &mut Vec<DependencyFinding>,
) -> ToolDependency {
    let executable = find_executable_on_path(tool);
    let found = executable.is_some();
    let version = executable.as_ref().and_then(|path| tool_version(path));
    let install_hint = (!found).then(|| install_hint(tool));
    if !found {
        next_actions.push(format!("install {tool}"));
        findings.push(dep_finding(
            "tool_missing",
            "error",
            "required tool is missing from PATH",
            "install the tool or update PATH before using the skill",
            json!({ "tool": tool, "install_hint": install_hint }),
        ));
    }
    ToolDependency {
        name: tool.to_string(),
        required: true,
        found,
        version,
        install_hint,
        source: source.to_string(),
    }
}

pub(super) fn probe_env(
    name: &str,
    source: &str,
    next_actions: &mut Vec<String>,
    findings: &mut Vec<DependencyFinding>,
) -> EnvDependency {
    let present = env::var_os(name).is_some();
    if !present {
        next_actions.push(format!("set {name}"));
        findings.push(dep_finding(
            "env_missing",
            "error",
            "required environment variable is missing",
            "set the environment variable without committing or printing its value",
            json!({ "env": name, "redacted": true }),
        ));
    }
    EnvDependency {
        name: name.to_string(),
        required: true,
        present,
        redacted: true,
        source: source.to_string(),
    }
}

pub(super) fn probe_mcp(
    _ctx: &AppContext,
    name: &str,
    source: &str,
    agent: Option<&str>,
    next_actions: &mut Vec<String>,
    findings: &mut Vec<DependencyFinding>,
) -> McpDependency {
    match agent.map(str::to_ascii_lowercase).as_deref() {
        Some("codex") => match codex_mcp_configured(name) {
            Some(configured) => {
                if !configured {
                    next_actions.push(format!("configure {name} MCP server"));
                    findings.push(dep_finding(
                        "mcp_missing",
                        "error",
                        "required MCP server is not configured",
                        "configure the MCP server for the selected agent",
                        json!({ "mcp": name, "agent": "codex" }),
                    ));
                }
                McpDependency {
                    name: name.to_string(),
                    required: true,
                    configured: json!(configured),
                    enabled: json!(configured),
                    source: source.to_string(),
                }
            }
            None => {
                next_actions.push(format!("check {name} MCP server for codex"));
                findings.push(dep_finding(
                    "mcp_status_unknown",
                    "warning",
                    "Codex MCP config could not be parsed",
                    "fix or inspect the Codex config before relying on MCP readiness",
                    json!({ "mcp": name, "agent": "codex" }),
                ));
                McpDependency {
                    name: name.to_string(),
                    required: true,
                    configured: json!("unknown"),
                    enabled: json!("unknown"),
                    source: source.to_string(),
                }
            }
        },
        Some(other) => {
            next_actions.push(format!("check {name} MCP server for {other}"));
            findings.push(dep_finding(
                "mcp_status_unknown",
                "warning",
                "MCP config detection is unsupported for this agent",
                "verify the MCP server manually for the selected agent",
                json!({ "mcp": name, "agent": other }),
            ));
            McpDependency {
                name: name.to_string(),
                required: true,
                configured: json!("unknown"),
                enabled: json!("unknown"),
                source: source.to_string(),
            }
        }
        None => {
            next_actions.push(format!("configure {name} MCP server"));
            findings.push(dep_finding(
                "mcp_missing",
                "error",
                "required MCP server cannot be verified without an agent config",
                "run skill deps with --agent or configure the MCP server",
                json!({ "mcp": name }),
            ));
            McpDependency {
                name: name.to_string(),
                required: true,
                configured: json!(false),
                enabled: json!(false),
                source: source.to_string(),
            }
        }
    }
}

pub(super) fn probe_network(
    required: &str,
    source: &str,
    next_actions: &mut Vec<String>,
    findings: &mut Vec<DependencyFinding>,
) -> NetworkDependency {
    let normalized = match required.trim().to_ascii_lowercase().as_str() {
        "required" | "true" | "yes" => "required",
        "optional" => "optional",
        _ => "none",
    };
    if normalized == "required" {
        next_actions.push("review network policy".to_string());
        findings.push(dep_finding(
            "network_required",
            "error",
            "skill declares required network access",
            "review network policy before running the skill",
            json!({ "source": source }),
        ));
    }
    NetworkDependency {
        required: normalized.to_string(),
        allowed_by_policy: false,
        source: Some(source.to_string()),
    }
}

fn tool_version(path: &Path) -> Option<String> {
    let mut child = Command::new(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().ok()? {
            let output = child.wait_with_output().ok()?;
            if !status.success() {
                return None;
            }
            let raw = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            return String::from_utf8_lossy(&raw)
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(|line| line.chars().take(120).collect());
        }
        if started.elapsed() >= VERSION_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn install_hint(tool: &str) -> String {
    match tool {
        "jq" => "brew install jq".to_string(),
        "uv" => "brew install uv".to_string(),
        "git" => "install git from your OS package manager".to_string(),
        "python" => "install Python 3".to_string(),
        "node" => "install Node.js".to_string(),
        "bash" | "sh" => "install a POSIX shell".to_string(),
        other => format!("install {other}"),
    }
}
