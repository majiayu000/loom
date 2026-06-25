use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::Context;
use serde::Serialize;
use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::cli::SkillPolicyArgs;
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_io, validate_policy_profile, validate_skill_name};
use super::provenance::{ProvenanceDigestStatus, provenance_digest_status};
use super::{App, CommandFailure};

const DEFAULT_POLICY_PROFILE: &str = "safe-capture";
const LARGE_FILE_BYTES: u64 = 1_000_000;
const MAX_FINDINGS_PER_KIND: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillPolicyReport {
    pub skill: String,
    pub policy_profile: String,
    pub allowed: bool,
    pub capabilities: SkillCapabilities,
    pub provenance: Option<ProvenanceDigestStatus>,
    pub summary: SkillPolicySummary,
    pub findings: Vec<SkillPolicyFinding>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SkillCapabilities {
    pub declared: bool,
    pub filesystem: BTreeMap<String, Vec<String>>,
    pub shell: BTreeMap<String, Vec<String>>,
    pub network: BTreeMap<String, Vec<String>>,
    pub secrets: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillPolicyFinding {
    pub id: String,
    pub risk_level: &'static str,
    pub blocks_projection: bool,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillPolicySummary {
    pub blocker_count: usize,
    pub high_risk_count: usize,
}

impl App {
    pub fn cmd_skill_policy(
        &self,
        args: &SkillPolicyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        if let Some(profile) = args.policy_profile.as_deref() {
            validate_policy_profile(profile)?;
        }
        let report = evaluate_skill_policy(
            &self.ctx,
            &args.skill,
            args.policy_profile
                .as_deref()
                .unwrap_or(DEFAULT_POLICY_PROFILE),
        )?;
        Ok((json!(report), Meta::default()))
    }
}

pub(crate) fn enforce_skill_policy(
    ctx: &AppContext,
    skill: &str,
    policy_profile: &str,
) -> std::result::Result<SkillPolicyReport, CommandFailure> {
    let report = evaluate_skill_policy(ctx, skill, policy_profile)?;
    if report.allowed {
        return Ok(report);
    }
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        format!(
            "policy profile '{}' blocked projection of skill '{}'",
            report.policy_profile, skill
        ),
    );
    failure.details = json!({
        "report": report,
        "suggested_actions": [
            "run loom skill policy <skill> --policy-profile <profile>",
            "review blocked findings and update the skill or choose an explicit audit-only policy"
        ]
    });
    Err(failure)
}

pub(crate) fn evaluate_skill_policy(
    ctx: &AppContext,
    skill: &str,
    policy_profile: &str,
) -> std::result::Result<SkillPolicyReport, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    validate_policy_profile(policy_profile)?;
    let skill_path = ctx.skill_path(skill);
    if !skill_path.is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }

    let mut findings = Vec::new();
    let policy_known = policy_profile_known(policy_profile);
    if !policy_known {
        push_raw_finding(
            &mut findings,
            "policy_profile_unknown",
            "medium",
            vec![format!("policy_profile={policy_profile}")],
        );
    }

    let capabilities = read_declared_capabilities(&skill_path, &mut findings)?;
    push_capability_findings(&capabilities, &mut findings);
    scan_skill_files(&skill_path, &mut findings)?;
    let provenance = provenance_digest_status(ctx, skill)?;
    match provenance.as_ref() {
        Some(status) if !status.matches => push_raw_finding(
            &mut findings,
            "provenance_digest_mismatch",
            "high",
            provenance_details(status),
        ),
        None => push_raw_finding(&mut findings, "provenance_missing", "medium", Vec::new()),
        _ => {}
    }

    apply_policy_profile(policy_profile, &mut findings);
    let blocker_count = findings
        .iter()
        .filter(|finding| finding.blocks_projection)
        .count();
    let high_risk_count = findings
        .iter()
        .filter(|finding| matches!(finding.risk_level, "high" | "critical"))
        .count();
    Ok(SkillPolicyReport {
        skill: skill.to_string(),
        policy_profile: policy_profile.to_string(),
        allowed: blocker_count == 0,
        capabilities,
        provenance,
        summary: SkillPolicySummary {
            blocker_count,
            high_risk_count,
        },
        findings,
    })
}

fn read_declared_capabilities(
    skill_path: &Path,
    findings: &mut Vec<SkillPolicyFinding>,
) -> std::result::Result<SkillCapabilities, CommandFailure> {
    let Some(entrypoint) = skill_entrypoint(skill_path) else {
        return Ok(SkillCapabilities::default());
    };
    let raw = fs::read_to_string(&entrypoint).map_err(map_io)?;
    let Some(frontmatter) = extract_frontmatter_lines(&raw) else {
        return Ok(SkillCapabilities::default());
    };
    let capabilities = parse_capabilities(&frontmatter);
    if capabilities.declared
        && capabilities.filesystem.is_empty()
        && capabilities.shell.is_empty()
        && capabilities.network.is_empty()
        && capabilities.secrets.is_empty()
    {
        push_raw_finding(
            findings,
            "capabilities_declared_empty",
            "low",
            vec![format!("entrypoint={}", entrypoint.display())],
        );
    }
    Ok(capabilities)
}

fn skill_entrypoint(skill_path: &Path) -> Option<std::path::PathBuf> {
    let strict = skill_path.join("SKILL.md");
    if strict.is_file() {
        return Some(strict);
    }
    let legacy = skill_path.join("skill.md");
    legacy.is_file().then_some(legacy)
}

fn extract_frontmatter_lines(raw: &str) -> Option<Vec<String>> {
    let mut lines = raw.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut out = Vec::new();
    for line in lines {
        if line == "---" {
            return Some(out);
        }
        out.push(line.to_string());
    }
    None
}

fn parse_capabilities(lines: &[String]) -> SkillCapabilities {
    let mut capabilities = SkillCapabilities::default();
    let mut in_capabilities = false;
    let mut section: Option<String> = None;
    let mut leaf: Option<String> = None;

    for raw in lines {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = raw.chars().take_while(|ch| *ch == ' ').count();
        if indent == 0 {
            let Some((key, _value)) = split_yaml_pair(trimmed) else {
                if in_capabilities {
                    break;
                }
                continue;
            };
            if key == "capabilities" {
                capabilities.declared = true;
                in_capabilities = true;
                section = None;
                leaf = None;
                continue;
            }
            if in_capabilities {
                break;
            }
        }
        if !in_capabilities {
            continue;
        }
        if indent == 2 {
            if let Some((key, _value)) = split_yaml_pair(trimmed) {
                section = Some(key.to_string());
                leaf = None;
            }
            continue;
        }
        if indent >= 4 {
            if let Some(item) = trimmed.strip_prefix("- ") {
                if let (Some(section), Some(leaf)) = (section.as_deref(), leaf.as_deref()) {
                    insert_capability_value(&mut capabilities, section, leaf, parse_scalar(item));
                }
                continue;
            }
            if let Some((key, value)) = split_yaml_pair(trimmed)
                && let Some(section) = section.as_deref()
            {
                leaf = Some(key.to_string());
                for value in parse_value_list(value) {
                    insert_capability_value(&mut capabilities, section, key, value);
                }
            }
        }
    }
    capabilities
}

fn split_yaml_pair(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

fn parse_value_list(value: &str) -> Vec<String> {
    let value = strip_inline_comment(value).trim();
    if value.is_empty() {
        return Vec::new();
    }
    if value.starts_with('[') && value.ends_with(']') {
        return value[1..value.len() - 1]
            .split(',')
            .map(parse_scalar)
            .filter(|item| !item.is_empty())
            .collect();
    }
    vec![parse_scalar(value)]
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect()
}

fn strip_inline_comment(value: &str) -> &str {
    value
        .split_once(" #")
        .map(|(left, _)| left)
        .unwrap_or(value)
}

fn parse_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn insert_capability_value(
    capabilities: &mut SkillCapabilities,
    section: &str,
    key: &str,
    value: String,
) {
    if value.is_empty() {
        return;
    }
    let map = match section {
        "filesystem" => &mut capabilities.filesystem,
        "shell" => &mut capabilities.shell,
        "network" => &mut capabilities.network,
        "secrets" => &mut capabilities.secrets,
        _ => return,
    };
    map.entry(key.to_string()).or_default().push(value);
}

fn push_capability_findings(
    capabilities: &SkillCapabilities,
    findings: &mut Vec<SkillPolicyFinding>,
) {
    for key in capabilities.filesystem.keys() {
        let risk = if key == "write" { "high" } else { "medium" };
        push_raw_finding(
            findings,
            &format!("capability_filesystem_{key}"),
            risk,
            Vec::new(),
        );
    }
    for key in capabilities.shell.keys() {
        push_raw_finding(
            findings,
            &format!("capability_shell_{key}"),
            "high",
            Vec::new(),
        );
    }
    for key in capabilities.network.keys() {
        push_raw_finding(
            findings,
            &format!("capability_network_{key}"),
            "high",
            Vec::new(),
        );
    }
    for key in capabilities.secrets.keys() {
        push_raw_finding(
            findings,
            &format!("capability_secrets_{key}"),
            "high",
            Vec::new(),
        );
    }
}

fn scan_skill_files(
    skill_path: &Path,
    findings: &mut Vec<SkillPolicyFinding>,
) -> std::result::Result<(), CommandFailure> {
    for entry in WalkDir::new(skill_path)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(map_io)?;
        let path = entry.path();
        let rel = path
            .strip_prefix(skill_path)
            .with_context(|| format!("strip {}", skill_path.display()))
            .map_err(map_io)?;
        let rel_string = rel.to_string_lossy().to_string();
        if entry.file_type().is_dir() {
            if is_generated_dir(&rel_string) {
                push_limited_finding(
                    findings,
                    make_finding("generated_artifact_dir", "high", path_details(&rel_string)),
                );
            }
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let metadata = entry.metadata().map_err(map_io)?;
        if metadata.len() > LARGE_FILE_BYTES {
            push_limited_finding(
                findings,
                make_finding(
                    "large_file",
                    "high",
                    vec![
                        format!("path={rel_string}"),
                        format!("bytes={}", metadata.len()),
                    ],
                ),
            );
        }
        if is_script_path(&rel_string) {
            push_limited_finding(
                findings,
                make_finding("script_file", "high", path_details(&rel_string)),
            );
        }
        if is_executable(&metadata) {
            push_limited_finding(
                findings,
                make_finding("executable_file", "high", path_details(&rel_string)),
            );
        }
        let bytes = read_prefix(path, 16 * 1024)?;
        if bytes.contains(&0) {
            push_limited_finding(
                findings,
                make_finding("binary_file", "high", path_details(&rel_string)),
            );
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        push_text_heuristics(&rel_string, &text, findings);
    }
    Ok(())
}

fn is_generated_dir(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            "node_modules" | "target" | "dist" | "build" | ".venv" | "__pycache__"
        )
    })
}

fn is_script_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "sh" | "bash" | "zsh" | "py" | "js" | "ts" | "rb" | "pl" | "ps1"
            )
        })
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

fn read_prefix(path: &Path, max_bytes: usize) -> std::result::Result<Vec<u8>, CommandFailure> {
    let mut file = fs::File::open(path).map_err(map_io)?;
    let mut buf = vec![0; max_bytes];
    let read = file.read(&mut buf).map_err(map_io)?;
    buf.truncate(read);
    Ok(buf)
}

fn push_text_heuristics(rel_path: &str, text: &str, findings: &mut Vec<SkillPolicyFinding>) {
    if text.contains("ignore previous instructions")
        || text.contains("ignore all previous instructions")
        || text.contains("system prompt")
        || text.contains("developer message")
        || text.contains("jailbreak")
    {
        push_limited_finding(
            findings,
            make_finding("prompt_injection_heuristic", "high", path_details(rel_path)),
        );
    }
    if (text.contains("curl ") || text.contains("wget ")) && text.contains("| sh") {
        push_limited_finding(
            findings,
            make_finding("shell_pipe_download", "high", path_details(rel_path)),
        );
    }
    if text.contains("eval(") || text.contains("exec(") {
        push_limited_finding(
            findings,
            make_finding(
                "dynamic_execution_heuristic",
                "high",
                path_details(rel_path),
            ),
        );
    }
}

fn policy_profile_known(profile: &str) -> bool {
    matches!(
        profile,
        "safe-capture" | "audit-only" | "deny-risky" | "strict"
    )
}

fn apply_policy_profile(profile: &str, findings: &mut [SkillPolicyFinding]) {
    for finding in findings {
        let blocks = match profile {
            "deny-risky" | "strict" => {
                matches!(finding.risk_level, "high" | "critical")
            }
            _ => false,
        };
        finding.blocks_projection = blocks;
    }
}

fn push_limited_finding(findings: &mut Vec<SkillPolicyFinding>, finding: SkillPolicyFinding) {
    if findings
        .iter()
        .filter(|existing| existing.id == finding.id)
        .count()
        >= MAX_FINDINGS_PER_KIND
    {
        return;
    }
    findings.push(finding);
}

fn push_raw_finding(
    findings: &mut Vec<SkillPolicyFinding>,
    id: &str,
    risk_level: &str,
    details: Vec<String>,
) {
    findings.push(make_finding(id, risk_level, details));
}

fn make_finding(id: &str, risk_level: &str, details: Vec<String>) -> SkillPolicyFinding {
    SkillPolicyFinding {
        id: id.to_string(),
        risk_level: match risk_level {
            "critical" => "critical",
            "high" => "high",
            "medium" => "medium",
            _ => "low",
        },
        blocks_projection: false,
        details,
    }
}

fn path_details(path: &str) -> Vec<String> {
    vec![format!("path={path}")]
}

fn provenance_details(status: &ProvenanceDigestStatus) -> Vec<String> {
    let mut details = vec![
        format!("recorded_digest={}", status.recorded_digest),
        format!("current_digest={}", status.current_digest),
        format!("lock_present={}", status.lock_present),
    ];
    if let Some(lock_digest) = &status.lock_digest {
        details.push(format!("lock_digest={lock_digest}"));
    }
    details
}
