use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use toml_edit::{DocumentMut, Item};
use walkdir::WalkDir;

use crate::cli::SkillDepsArgs;
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_io, validate_skill_name};
use super::skill_lint::frontmatter::parse_skill_frontmatter;
use super::skill_lint::{SkillLintFinding, SkillLintSummary};
use super::{App, CommandFailure};

pub(crate) mod support;
use support::{
    codex_mcp_configured, contains_word_token, find_executable_on_path, yaml_dependency_values,
};

const VERSION_TIMEOUT: Duration = Duration::from_millis(250);
const SCRIPT_SCAN_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillDependencyReport {
    pub skill: String,
    pub dependencies: DependencyGroups,
    pub ready: bool,
    pub status: String,
    pub next_actions: Vec<String>,
    pub findings: Vec<DependencyFinding>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct DependencyGroups {
    pub tools: Vec<ToolDependency>,
    pub mcp: Vec<McpDependency>,
    pub env: Vec<EnvDependency>,
    pub network: NetworkDependency,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ToolDependency {
    pub name: String,
    pub required: bool,
    pub found: bool,
    pub version: Option<String>,
    pub install_hint: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct McpDependency {
    pub name: String,
    pub required: bool,
    pub configured: Value,
    pub enabled: Value,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct EnvDependency {
    pub name: String,
    pub required: bool,
    pub present: bool,
    pub redacted: bool,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NetworkDependency {
    pub required: String,
    pub allowed_by_policy: bool,
    pub source: Option<String>,
}

impl Default for NetworkDependency {
    fn default() -> Self {
        Self {
            required: "none".to_string(),
            allowed_by_policy: false,
            source: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DependencyFinding {
    pub id: String,
    pub severity: String,
    pub message: String,
    pub suggested_action: String,
    pub details: Value,
}

#[derive(Default)]
struct DependencyDeclarations {
    tools: BTreeMap<String, String>,
    mcp: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
    network: Option<(String, String)>,
    sources: BTreeSet<String>,
    findings: Vec<DependencyFinding>,
}

impl App {
    pub fn cmd_skill_deps(
        &self,
        args: &SkillDepsArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let report = skill_dependency_report(
            &self.ctx,
            &args.skill,
            args.agent.as_deref(),
            args.workspace.as_deref(),
        )?;
        Ok((json!(report), Meta::default()))
    }
}

pub(crate) fn skill_dependency_report(
    ctx: &AppContext,
    skill: &str,
    agent: Option<&str>,
    _workspace: Option<&Path>,
) -> std::result::Result<SkillDependencyReport, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let skill_path = ctx.skill_path(skill);
    if !skill_path.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }

    let declarations = collect_declarations(&skill_path, agent)?;
    let mut next_actions = Vec::new();
    let mut findings = declarations.findings.clone();
    let tools = declarations
        .tools
        .iter()
        .map(|(tool, source)| probe_tool(tool, source, &mut next_actions, &mut findings))
        .collect::<Vec<_>>();
    let env = declarations
        .env
        .iter()
        .map(|(name, source)| probe_env(name, source, &mut next_actions, &mut findings))
        .collect::<Vec<_>>();
    let mcp = declarations
        .mcp
        .iter()
        .map(|(name, source)| probe_mcp(ctx, name, source, agent, &mut next_actions, &mut findings))
        .collect::<Vec<_>>();
    let network = declarations
        .network
        .as_ref()
        .map(|(required, source)| probe_network(required, source, &mut next_actions, &mut findings))
        .unwrap_or_default();

    let has_error = findings.iter().any(|finding| finding.severity == "error");
    let has_unknown = findings
        .iter()
        .any(|finding| finding.id == "mcp_status_unknown");
    let ready = !has_error && !has_unknown;
    let status = if has_error {
        "blocked"
    } else if has_unknown {
        "unknown"
    } else {
        "ready"
    };

    Ok(SkillDependencyReport {
        skill: skill.to_string(),
        dependencies: DependencyGroups {
            tools,
            mcp,
            env,
            network,
        },
        ready,
        status: status.to_string(),
        next_actions,
        findings,
        sources: declarations.sources.into_iter().collect(),
    })
}

pub(crate) fn append_dependency_lint_findings(
    report: &mut super::SkillLintReport,
    deps: &SkillDependencyReport,
) {
    if deps.sources.is_empty() {
        report.findings.push(lint_finding(
            "quality_dependencies_undeclared",
            "warning",
            "skill does not declare runtime dependencies",
            "add loom.skill.toml or SKILL.md metadata for required tools, env, MCP, and network",
            json!({}),
        ));
    }
    for finding in &deps.findings {
        let severity = if finding.severity == "error" {
            "warning"
        } else {
            finding.severity.as_str()
        };
        report.findings.push(lint_finding(
            &format!("quality_dependency_{}", finding.id),
            severity,
            &finding.message,
            &finding.suggested_action,
            finding.details.clone(),
        ));
    }
    refresh_lint_summary(report);
}

fn collect_declarations(
    skill_path: &Path,
    agent: Option<&str>,
) -> std::result::Result<DependencyDeclarations, CommandFailure> {
    let mut declarations = DependencyDeclarations::default();
    read_manifest(skill_path, &mut declarations);
    read_frontmatter(skill_path, &mut declarations);
    read_scripts(skill_path, &mut declarations)?;
    read_agent_metadata(skill_path, agent, &mut declarations);
    Ok(declarations)
}

fn read_manifest(skill_path: &Path, declarations: &mut DependencyDeclarations) {
    let path = skill_path.join("loom.skill.toml");
    if !path.is_file() {
        return;
    }
    declarations.sources.insert("loom.skill.toml".to_string());
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            declarations.findings.push(dep_finding(
                "manifest_read_failed",
                "warning",
                "loom.skill.toml could not be read",
                "fix manifest permissions or remove the unreadable file",
                json!({ "error": err.to_string() }),
            ));
            return;
        }
    };
    let doc = match raw.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(err) => {
            declarations.findings.push(dep_finding(
                "manifest_toml_invalid",
                "warning",
                "loom.skill.toml did not parse",
                "fix TOML syntax before relying on dependency readiness",
                json!({ "error": err.to_string() }),
            ));
            return;
        }
    };
    add_array_items(
        doc.get("requires_tools"),
        "loom.skill.toml",
        &mut declarations.tools,
    );
    add_array_items(
        doc.get("requires_mcp"),
        "loom.skill.toml",
        &mut declarations.mcp,
    );
    add_array_items(
        doc.get("requires_env"),
        "loom.skill.toml",
        &mut declarations.env,
    );
    if let Some(network) = doc.get("network").and_then(Item::as_str) {
        set_network(network, "loom.skill.toml", declarations);
    }
}

fn read_frontmatter(skill_path: &Path, declarations: &mut DependencyDeclarations) {
    let entrypoint = if skill_path.join("SKILL.md").is_file() {
        skill_path.join("SKILL.md")
    } else {
        skill_path.join("skill.md")
    };
    if !entrypoint.is_file() {
        return;
    }
    let parsed = match parse_skill_frontmatter(&entrypoint) {
        Ok(parsed) => parsed.frontmatter,
        Err(err) => {
            declarations.findings.push(dep_finding(
                "frontmatter_parse_failed",
                "warning",
                "SKILL.md frontmatter did not parse for dependency metadata",
                "fix YAML frontmatter dependency metadata",
                json!({ "error": err }),
            ));
            return;
        }
    };
    add_csv_metadata(
        parsed.metadata.get("loom.requires_tools"),
        "SKILL.md metadata",
        &mut declarations.tools,
    );
    add_csv_metadata(
        parsed.metadata.get("loom.requires_mcp"),
        "SKILL.md metadata",
        &mut declarations.mcp,
    );
    add_csv_metadata(
        parsed.metadata.get("loom.requires_env"),
        "SKILL.md metadata",
        &mut declarations.env,
    );
    if let Some(network) = parsed.metadata.get("loom.network") {
        set_network(network, "SKILL.md metadata", declarations);
    }
    if let Some(compatibility) = parsed.compatibility {
        declarations
            .sources
            .insert("SKILL.md compatibility".to_string());
        infer_from_text(
            &compatibility_to_text(&compatibility),
            "SKILL.md compatibility",
            declarations,
        );
    }
}

fn read_scripts(
    skill_path: &Path,
    declarations: &mut DependencyDeclarations,
) -> std::result::Result<(), CommandFailure> {
    let scripts = skill_path.join("scripts");
    if !scripts.is_dir() {
        return Ok(());
    }
    for entry in WalkDir::new(&scripts)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().is_file() {
            continue;
        }
        declarations.sources.insert("scripts".to_string());
        let bytes = read_prefix(entry.path(), SCRIPT_SCAN_BYTES)?;
        if bytes.contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        if text.starts_with("#!") {
            infer_shebang_tool(&text, declarations);
        }
        infer_from_text(&text, "scripts", declarations);
    }
    Ok(())
}

fn read_agent_metadata(
    skill_path: &Path,
    agent: Option<&str>,
    declarations: &mut DependencyDeclarations,
) {
    let agents_dir = skill_path.join("agents");
    if !agents_dir.is_dir() {
        return;
    }
    let mut paths = Vec::new();
    if let Some(agent) = agent {
        paths.push(agents_dir.join(format!("{agent}.yaml")));
        paths.push(agents_dir.join(format!("{agent}.yml")));
    } else if let Ok(entries) = fs::read_dir(&agents_dir) {
        paths.extend(entries.flatten().map(|entry| entry.path()));
    }
    for path in paths.into_iter().filter(|path| path.is_file()) {
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        declarations.sources.insert("agent metadata".to_string());
        for (key, value) in yaml_dependency_values(&raw) {
            match key.as_str() {
                "requires_tools" => {
                    add_csv_value(&value, "agent metadata", &mut declarations.tools)
                }
                "requires_mcp" => add_csv_value(&value, "agent metadata", &mut declarations.mcp),
                "requires_env" => add_csv_value(&value, "agent metadata", &mut declarations.env),
                "network" => set_network(value.trim(), "agent metadata", declarations),
                _ => {}
            }
        }
    }
}

fn probe_tool(
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

fn probe_env(
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

fn probe_mcp(
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

fn probe_network(
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

fn add_array_items(item: Option<&Item>, source: &str, target: &mut BTreeMap<String, String>) {
    if let Some(array) = item.and_then(Item::as_array) {
        for value in array.iter().filter_map(|value| value.as_str()) {
            insert_requirement(value, source, target);
        }
    }
}

fn add_csv_metadata(value: Option<&String>, source: &str, target: &mut BTreeMap<String, String>) {
    if let Some(value) = value {
        add_csv_value(value, source, target);
    }
}

fn add_csv_value(value: &str, source: &str, target: &mut BTreeMap<String, String>) {
    for item in value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
    {
        insert_requirement(
            item.trim().trim_matches('"').trim_matches('\''),
            source,
            target,
        );
    }
}

fn insert_requirement(value: &str, source: &str, target: &mut BTreeMap<String, String>) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    target
        .entry(value.to_string())
        .or_insert_with(|| source.to_string());
}

fn set_network(value: &str, source: &str, declarations: &mut DependencyDeclarations) {
    declarations.sources.insert(source.to_string());
    declarations.network = Some((value.trim().to_ascii_lowercase(), source.to_string()));
}

fn infer_from_text(text: &str, source: &str, declarations: &mut DependencyDeclarations) {
    let lower = text.to_ascii_lowercase();
    for (needle, tool) in [
        ("git", "git"),
        ("jq", "jq"),
        ("python", "python"),
        ("python3", "python"),
        ("node", "node"),
        ("uv", "uv"),
    ] {
        if contains_word_token(&lower, needle) {
            insert_requirement(tool, source, &mut declarations.tools);
            declarations.sources.insert(source.to_string());
        }
    }
    if lower.contains("github mcp") || lower.contains("github server") {
        insert_requirement("github", source, &mut declarations.mcp);
        declarations.sources.insert(source.to_string());
    }
    if lower.contains("filesystem mcp") {
        insert_requirement("filesystem", source, &mut declarations.mcp);
        declarations.sources.insert(source.to_string());
    }
    if contains_word_token(&lower, "curl")
        || contains_word_token(&lower, "wget")
        || contains_word_token(&lower, "network")
    {
        set_network("required", source, declarations);
    }
}

fn infer_shebang_tool(text: &str, declarations: &mut DependencyDeclarations) {
    let first = text.lines().next().unwrap_or_default();
    for (needle, tool) in [
        ("python", "python"),
        ("node", "node"),
        ("bash", "bash"),
        ("/sh", "sh"),
    ] {
        if first.contains(needle) {
            insert_requirement(tool, "scripts", &mut declarations.tools);
            declarations.sources.insert("scripts".to_string());
        }
    }
}

fn compatibility_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
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

fn read_prefix(path: &Path, max_bytes: usize) -> std::result::Result<Vec<u8>, CommandFailure> {
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(map_io)?;
    let mut buf = vec![0; max_bytes];
    let read = file.read(&mut buf).map_err(map_io)?;
    buf.truncate(read);
    Ok(buf)
}

fn lint_finding(
    id: &str,
    severity: &str,
    message: &str,
    suggested_action: &str,
    details: Value,
) -> SkillLintFinding {
    SkillLintFinding {
        id: id.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
        suggested_action: suggested_action.to_string(),
        fixable: false,
        details,
    }
}

fn refresh_lint_summary(report: &mut super::SkillLintReport) {
    let error_count = report
        .findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    let warning_count = report
        .findings
        .iter()
        .filter(|finding| finding.severity == "warning")
        .count();
    report.valid = error_count == 0;
    report.compatible = report.compatible && error_count == 0;
    let quality = report
        .findings
        .iter()
        .filter(|finding| finding.id.starts_with("quality_"))
        .collect::<Vec<_>>();
    report.sections.quality.status = if quality.iter().any(|finding| finding.severity == "error") {
        "error"
    } else if quality.iter().any(|finding| finding.severity == "warning") {
        "warning"
    } else {
        "pass"
    }
    .to_string();
    report.sections.quality.findings = quality.iter().map(|finding| finding.id.clone()).collect();
    report.summary = SkillLintSummary {
        error_count,
        warning_count,
        fixable_count: report
            .findings
            .iter()
            .filter(|finding| finding.fixable)
            .count(),
    };
}

fn dep_finding(
    id: &str,
    severity: &str,
    message: &str,
    suggested_action: &str,
    details: Value,
) -> DependencyFinding {
    DependencyFinding {
        id: id.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
        suggested_action: suggested_action.to_string(),
        details,
    }
}
