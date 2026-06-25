use std::collections::{BTreeMap, BTreeSet};
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
    pub policy_known: bool,
    pub allowed: bool,
    pub capabilities: SkillCapabilities,
    pub provenance: Option<ProvenanceDigestStatus>,
    pub summary: SkillPolicySummary,
    pub findings: Vec<SkillPolicyFinding>,
    pub limitations: Vec<String>,
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
    pub severity: String,
    pub risk_level: String,
    pub category: String,
    pub message: String,
    pub suggested_action: String,
    pub blocks_projection: bool,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillPolicySummary {
    pub finding_count: usize,
    pub warning_count: usize,
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
            "policy",
            &format!(
                "policy profile '{}' has no built-in enforcement rules",
                policy_profile
            ),
            "define organization-side handling for this profile or use audit-only/deny-risky",
            json!({ "policy_profile": policy_profile }),
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
            "provenance",
            "current skill source digest does not match recorded provenance or loom.lock",
            "review the source diff, then run skill provenance refresh only after accepting the change",
            json!({
                "recorded_digest": status.recorded_digest,
                "current_digest": status.current_digest,
                "lock_digest": status.lock_digest,
                "lock_present": status.lock_present,
            }),
        ),
        None => push_raw_finding(
            &mut findings,
            "provenance_missing",
            "medium",
            "provenance",
            "skill has no recorded source provenance",
            "re-import the skill with skill add or record provenance before relying on digest checks",
            json!({}),
        ),
        _ => {}
    }

    apply_policy_profile(policy_profile, &mut findings);
    let blocker_count = findings
        .iter()
        .filter(|finding| finding.blocks_projection)
        .count();
    let warning_count = findings
        .iter()
        .filter(|finding| finding.severity == "warning")
        .count();
    let high_risk_count = findings
        .iter()
        .filter(|finding| matches!(finding.risk_level.as_str(), "high" | "critical"))
        .count();
    Ok(SkillPolicyReport {
        skill: skill.to_string(),
        policy_profile: policy_profile.to_string(),
        policy_known,
        allowed: blocker_count == 0,
        capabilities,
        provenance,
        summary: SkillPolicySummary {
            finding_count: findings.len(),
            warning_count,
            blocker_count,
            high_risk_count,
        },
        findings,
        limitations: vec![
            "policy checks are heuristic signals, not a sandbox or malware verdict".to_string(),
            "prompt-injection findings are warnings that require human review".to_string(),
        ],
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
            "capabilities",
            "capabilities frontmatter is present but no supported capability keys were parsed",
            "use filesystem, shell, network, or secrets capability sections",
            json!({ "entrypoint": entrypoint.display().to_string() }),
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
    for (key, values) in &capabilities.filesystem {
        let risk = if key == "write" { "high" } else { "medium" };
        push_raw_finding(
            findings,
            &format!("capability_filesystem_{key}"),
            risk,
            "capabilities",
            &format!("skill declares filesystem {key} capability"),
            "review whether the target agent should receive this filesystem capability",
            json!({ "values": values }),
        );
    }
    for (key, values) in &capabilities.shell {
        push_raw_finding(
            findings,
            &format!("capability_shell_{key}"),
            "high",
            "capabilities",
            &format!("skill declares shell {key} capability"),
            "review command allowlist before projection",
            json!({ "values": values }),
        );
    }
    for (key, values) in &capabilities.network {
        push_raw_finding(
            findings,
            &format!("capability_network_{key}"),
            "high",
            "capabilities",
            &format!("skill declares network {key} capability"),
            "review domain allowlist before projection",
            json!({ "values": values }),
        );
    }
    for (key, values) in &capabilities.secrets {
        push_raw_finding(
            findings,
            &format!("capability_secrets_{key}"),
            "high",
            "capabilities",
            &format!("skill declares secrets {key} capability"),
            "review secret exposure before projection",
            json!({ "values": values }),
        );
    }
}

fn scan_skill_files(
    skill_path: &Path,
    findings: &mut Vec<SkillPolicyFinding>,
) -> std::result::Result<(), CommandFailure> {
    let mut generated_dirs = BTreeSet::new();
    let mut finding_counts: BTreeMap<String, usize> = BTreeMap::new();
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
            if is_generated_dir(&rel_string) && generated_dirs.insert(rel_string.clone()) {
                push_limited_finding(
                    findings,
                    &mut finding_counts,
                    make_finding(
                        "generated_artifact_dir",
                        "high",
                        "artifact",
                        "skill contains a generated artifact directory",
                        "remove generated artifacts from the skill source before projection",
                        json!({ "path": rel_string }),
                    ),
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
                &mut finding_counts,
                make_finding(
                    "large_file",
                    "high",
                    "artifact",
                    "skill contains a very large file",
                    "remove large generated or binary assets unless explicitly required",
                    json!({ "path": rel_string, "bytes": metadata.len() }),
                ),
            );
        }
        if is_script_path(&rel_string) {
            push_limited_finding(
                findings,
                &mut finding_counts,
                make_finding(
                    "script_file",
                    "high",
                    "executable_content",
                    "skill contains a script file",
                    "review script contents and use deny-risky policy until approved",
                    json!({ "path": rel_string }),
                ),
            );
        }
        if is_executable(&metadata) {
            push_limited_finding(
                findings,
                &mut finding_counts,
                make_finding(
                    "executable_file",
                    "high",
                    "executable_content",
                    "skill contains an executable file",
                    "review executable content before projection",
                    json!({ "path": rel_string }),
                ),
            );
        }
        let bytes = read_prefix(path, 16 * 1024)?;
        if bytes.contains(&0) {
            push_limited_finding(
                findings,
                &mut finding_counts,
                make_finding(
                    "binary_file",
                    "high",
                    "executable_content",
                    "skill contains binary-looking file content",
                    "remove or explicitly review binary files before projection",
                    json!({ "path": rel_string }),
                ),
            );
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
        push_text_heuristics(&rel_string, &text, findings, &mut finding_counts);
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

fn push_text_heuristics(
    rel_path: &str,
    text: &str,
    findings: &mut Vec<SkillPolicyFinding>,
    counts: &mut BTreeMap<String, usize>,
) {
    if text.contains("ignore previous instructions")
        || text.contains("ignore all previous instructions")
        || text.contains("system prompt")
        || text.contains("developer message")
        || text.contains("jailbreak")
    {
        push_limited_finding(
            findings,
            counts,
            make_finding(
                "prompt_injection_heuristic",
                "high",
                "content",
                "skill text matched a prompt-injection heuristic",
                "review manually; this heuristic is not a safety verdict",
                json!({ "path": rel_path }),
            ),
        );
    }
    if (text.contains("curl ") || text.contains("wget ")) && text.contains("| sh") {
        push_limited_finding(
            findings,
            counts,
            make_finding(
                "shell_pipe_download",
                "high",
                "executable_content",
                "skill content appears to pipe downloaded content into a shell",
                "remove this pattern or approve it through explicit review",
                json!({ "path": rel_path }),
            ),
        );
    }
    if text.contains("eval(") || text.contains("exec(") {
        push_limited_finding(
            findings,
            counts,
            make_finding(
                "dynamic_execution_heuristic",
                "high",
                "executable_content",
                "skill content matched a dynamic execution heuristic",
                "review dynamic execution before projection",
                json!({ "path": rel_path }),
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
                matches!(finding.risk_level.as_str(), "high" | "critical")
            }
            _ => false,
        };
        finding.blocks_projection = blocks;
        finding.severity = if blocks { "blocker" } else { "warning" }.to_string();
    }
}

fn push_limited_finding(
    findings: &mut Vec<SkillPolicyFinding>,
    counts: &mut BTreeMap<String, usize>,
    finding: SkillPolicyFinding,
) {
    let count = counts.entry(finding.id.clone()).or_default();
    if *count >= MAX_FINDINGS_PER_KIND {
        return;
    }
    *count += 1;
    findings.push(finding);
}

fn push_raw_finding(
    findings: &mut Vec<SkillPolicyFinding>,
    id: &str,
    risk_level: &str,
    category: &str,
    message: &str,
    suggested_action: &str,
    details: Value,
) {
    findings.push(make_finding(
        id,
        risk_level,
        category,
        message,
        suggested_action,
        details,
    ));
}

fn make_finding(
    id: &str,
    risk_level: &str,
    category: &str,
    message: &str,
    suggested_action: &str,
    details: Value,
) -> SkillPolicyFinding {
    SkillPolicyFinding {
        id: id.to_string(),
        severity: "warning".to_string(),
        risk_level: risk_level.to_string(),
        category: category.to_string(),
        message: message.to_string(),
        suggested_action: suggested_action.to_string(),
        blocks_projection: false,
        details,
    }
}
