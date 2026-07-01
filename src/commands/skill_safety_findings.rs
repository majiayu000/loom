use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::Path;

use walkdir::WalkDir;

use super::CommandFailure;
use super::helpers::map_io;
use super::skill_policy::{SkillPolicyFinding, SkillPolicyReport};
use super::skill_safety::{SafetyFinding, SafetySummary, SkillTrustMetadata};

const MAX_SAFETY_SCAN_BYTES: usize = 16 * 1024;

struct TextSafetyCheck {
    id: &'static str,
    severity: &'static str,
    script_only: bool,
    needles: &'static [&'static str],
    message: &'static str,
    suggested_action: &'static str,
}

const PROMPT_INJECTION_NEEDLES: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous instructions",
    "developer message",
    "system prompt",
];
const OVERTRIGGER_NEEDLES: &[&str] = &["always use", "use for every"];
const INSTRUCTION_SECRET_NEEDLES: &[&str] = &[
    "exfiltrat",
    "send secrets",
    "read secrets",
    "id_rsa",
    ".env",
];
const POLICY_BYPASS_NEEDLES: &[&str] = &[
    "bypass approval",
    "disable sandbox",
    "without asking permission",
];
const SCRIPT_NETWORK_NEEDLES: &[&str] = &[
    "curl ",
    "wget ",
    "nc ",
    "ssh ",
    "scp ",
    "rsync ",
    "requests.",
];
const SCRIPT_SECRET_NEEDLES: &[&str] = &[
    ".env",
    "id_rsa",
    ".aws/credentials",
    "github_token",
    "keychain",
];
const SCRIPT_DESTRUCTIVE_NEEDLES: &[&str] = &[
    "rm -rf",
    "mkfs",
    "diskutil erase",
    "git push --force",
    "chmod -r 777",
    "chown -r",
];
const SCRIPT_SHELL_NEEDLES: &[&str] = &[
    "eval ",
    "eval(",
    "exec(",
    "shell=true",
    "child_process.exec",
];

const TEXT_SAFETY_CHECKS: &[TextSafetyCheck] = &[
    TextSafetyCheck {
        id: "instruction_prompt_injection",
        severity: "high",
        script_only: false,
        needles: PROMPT_INJECTION_NEEDLES,
        message: "instruction appears to target higher-priority prompts",
        suggested_action: "remove prompt-injection-like language before activation",
    },
    TextSafetyCheck {
        id: "description_overtrigger",
        severity: "medium",
        script_only: false,
        needles: OVERTRIGGER_NEEDLES,
        message: "description or instructions may over-trigger unrelated tasks",
        suggested_action: "narrow the trigger language to the intended workflow",
    },
    TextSafetyCheck {
        id: "instruction_secret_exfiltration",
        severity: "critical",
        script_only: false,
        needles: INSTRUCTION_SECRET_NEEDLES,
        message: "instruction references reading or exposing secrets",
        suggested_action: "remove secret-reading instructions and document required credentials explicitly",
    },
    TextSafetyCheck {
        id: "instruction_policy_bypass",
        severity: "high",
        script_only: false,
        needles: POLICY_BYPASS_NEEDLES,
        message: "instruction appears to bypass approval or sandbox policy",
        suggested_action: "remove policy-bypass instructions",
    },
    TextSafetyCheck {
        id: "script_network_access",
        severity: "high",
        script_only: true,
        needles: SCRIPT_NETWORK_NEEDLES,
        message: "script invokes network-capable command",
        suggested_action: "review destination, pin downloads, and avoid network access when possible",
    },
    TextSafetyCheck {
        id: "script_secret_read",
        severity: "critical",
        script_only: true,
        needles: SCRIPT_SECRET_NEEDLES,
        message: "script references local secret material",
        suggested_action: "remove secret reads or require explicit user-provided environment variables",
    },
    TextSafetyCheck {
        id: "script_destructive_command",
        severity: "critical",
        script_only: true,
        needles: SCRIPT_DESTRUCTIVE_NEEDLES,
        message: "script contains destructive command patterns",
        suggested_action: "remove destructive commands or require an explicit reviewed workflow",
    },
    TextSafetyCheck {
        id: "script_shell_injection",
        severity: "high",
        script_only: true,
        needles: SCRIPT_SHELL_NEEDLES,
        message: "script contains dynamic shell execution pattern",
        suggested_action: "replace dynamic shell execution with fixed argument arrays",
    },
];

pub(super) fn policy_findings(policy: &SkillPolicyReport) -> Vec<SafetyFinding> {
    policy
        .findings
        .iter()
        .map(policy_finding_to_safety)
        .collect()
}

fn policy_finding_to_safety(finding: &SkillPolicyFinding) -> SafetyFinding {
    let id = match finding.id.as_str() {
        "prompt_injection_heuristic" => "instruction_prompt_injection",
        "shell_pipe_download" => "script_network_access",
        "dynamic_execution_heuristic" => "script_shell_injection",
        id => id,
    };
    SafetyFinding {
        id: id.to_string(),
        severity: finding.risk_level.to_string(),
        path: detail_value(&finding.details, "path="),
        line: None,
        message: safety_message(id).to_string(),
        suggested_action: suggested_action(id).to_string(),
    }
}

pub(super) fn scan_skill_safety_files(
    skill_path: &Path,
    findings: &mut Vec<SafetyFinding>,
) -> std::result::Result<(), CommandFailure> {
    for entry in WalkDir::new(skill_path)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let rel = path
            .strip_prefix(skill_path)
            .map_err(map_io)?
            .to_string_lossy()
            .to_string();
        let bytes = read_prefix(path, MAX_SAFETY_SCAN_BYTES)?;
        if bytes.contains(&0) {
            continue;
        }
        let raw = String::from_utf8_lossy(&bytes);
        push_text_safety_findings(&rel, &raw, findings);
    }
    Ok(())
}

pub(super) fn push_text_safety_findings(rel: &str, raw: &str, findings: &mut Vec<SafetyFinding>) {
    let text = raw.to_ascii_lowercase();
    let is_script = is_safety_script_path(rel);
    for check in TEXT_SAFETY_CHECKS {
        if check.script_only && !is_script {
            continue;
        }
        if contains_any(&text, check.needles) {
            push_finding(
                findings,
                check.id,
                check.severity,
                Some(rel.to_string()),
                first_matching_line(raw, check.needles),
                check.message,
                check.suggested_action,
            );
        }
    }
}

pub(super) fn push_trust_findings(trust: &SkillTrustMetadata, findings: &mut Vec<SafetyFinding>) {
    if trust.quarantined || trust.trust == "quarantined" {
        push_finding(
            findings,
            "trust_quarantined",
            "critical",
            None,
            None,
            "skill is quarantined",
            "run loom skill unquarantine <skill> only after review",
        );
    } else if trust.trust == "blocked" {
        push_finding(
            findings,
            "trust_blocked",
            "critical",
            None,
            None,
            "skill is blocked by trust metadata",
            "change trust only after a safety review",
        );
    }
}

pub(super) fn summarize_findings(findings: &[SafetyFinding]) -> SafetySummary {
    let mut summary = SafetySummary::default();
    for finding in findings {
        match finding.severity.as_str() {
            "critical" => summary.critical += 1,
            "high" => summary.high += 1,
            "medium" => summary.medium += 1,
            _ => summary.low += 1,
        }
    }
    summary
}

pub(super) fn dedupe_findings(findings: &mut Vec<SafetyFinding>) {
    let mut seen = BTreeSet::new();
    findings.retain(|finding| {
        seen.insert((
            finding.id.clone(),
            finding.path.clone(),
            finding.line,
            finding.message.clone(),
        ))
    });
}

pub(super) fn push_finding(
    findings: &mut Vec<SafetyFinding>,
    id: &str,
    severity: &str,
    path: Option<String>,
    line: Option<usize>,
    message: &str,
    suggested_action: &str,
) {
    findings.push(SafetyFinding {
        id: id.to_string(),
        severity: severity.to_string(),
        path,
        line,
        message: message.to_string(),
        suggested_action: suggested_action.to_string(),
    });
}

pub(super) fn is_metadata_path(path: &str) -> bool {
    matches!(path, "SKILL.md" | "skill.md" | "loom.skill.toml")
}

pub(super) fn is_security_relevant_path(path: &str) -> bool {
    is_metadata_path(path)
        || is_safety_script_path(path)
        || path.starts_with("scripts/")
        || path.starts_with("references/")
        || path.ends_with(".md")
}

fn detail_value(details: &[String], prefix: &str) -> Option<String> {
    details
        .iter()
        .find_map(|detail| detail.strip_prefix(prefix).map(ToString::to_string))
}

fn first_matching_line(raw: &str, needles: &[&str]) -> Option<usize> {
    raw.lines().enumerate().find_map(|(index, line)| {
        let line = line.to_ascii_lowercase();
        needles
            .iter()
            .any(|needle| line.contains(&needle.to_ascii_lowercase()))
            .then_some(index + 1)
    })
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn is_safety_script_path(path: &str) -> bool {
    if path.starts_with("scripts/") {
        return true;
    }
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

fn read_prefix(path: &Path, max_bytes: usize) -> std::result::Result<Vec<u8>, CommandFailure> {
    let mut file = fs::File::open(path).map_err(map_io)?;
    let mut buf = vec![0; max_bytes];
    let read = file.read(&mut buf).map_err(map_io)?;
    buf.truncate(read);
    Ok(buf)
}

fn safety_message(id: &str) -> &'static str {
    match id {
        "instruction_prompt_injection" => "instruction appears to target higher-priority prompts",
        "script_network_access" => "network-capable script or instruction detected",
        "script_shell_injection" => "dynamic shell execution pattern detected",
        "provenance_missing" => "source provenance is missing",
        "provenance_digest_mismatch" => "source provenance digest does not match",
        "trust_blocked" => "skill is blocked by trust metadata",
        "trust_quarantined" => "skill is quarantined",
        _ => "policy or safety finding detected",
    }
}

fn suggested_action(id: &str) -> &'static str {
    match id {
        "instruction_prompt_injection" => "remove prompt-injection-like language",
        "script_network_access" => "review network destination and pin downloads",
        "script_shell_injection" => "replace dynamic execution with fixed argument arrays",
        "provenance_missing" => "record source provenance before release or team activation",
        "provenance_digest_mismatch" => "refresh or investigate provenance before activation",
        "trust_blocked" => "change trust only after a safety review",
        "trust_quarantined" => "unquarantine only after review",
        _ => "review the finding before activation",
    }
}
