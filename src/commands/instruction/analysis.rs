use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::InstructionMigrationTarget;
use crate::commands::helpers::{map_arg, map_io, validate_skill_name};
use crate::commands::{App, CommandFailure};
use crate::types::ErrorCode;

use super::model::{InstructionSurface, ScanResult};

#[derive(Debug, Clone)]
pub(super) struct ContentSignals {
    pub(super) signals: Vec<String>,
    pub(super) contains_skill_like_workflow: bool,
    pub(super) suggested_action: String,
    pub(super) warnings: Vec<String>,
    pub(super) line_count: usize,
    pub(super) size_bytes: u64,
    pub(super) path_patterns: Vec<String>,
}

pub(super) struct DoctorSkill {
    pub(super) name: String,
    pub(super) path: String,
    body: String,
}

pub(super) fn doctor_findings(
    scan: &ScanResult,
    skill: Option<&DoctorSkill>,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    let mut findings = Vec::new();

    for unsupported in &scan.unsupported_surfaces {
        findings.push(json!({
            "id": "missing_adapter_metadata",
            "severity": "warning",
            "agent": unsupported.agent,
            "reason": unsupported.reason,
            "suggested_action": "unsupported",
        }));
    }

    for surface in &scan.surfaces {
        let body = read_text_body(Path::new(&surface.path))?;
        if surface.always_on && surface.contains_skill_like_workflow {
            findings.push(json!({
                "id": "shadowing_risk",
                "severity": "warning",
                "instruction_id": surface.instruction_id,
                "path": surface.path,
                "scope": surface.scope,
                "affected_skill": skill.map(|skill| skill.name.as_str()),
                "suggested_action": "extract-skill",
            }));
        }
        if surface.line_count > 120 {
            findings.push(json!({
                "id": "prompt_budget_risk",
                "severity": "warning",
                "instruction_id": surface.instruction_id,
                "path": surface.path,
                "line_count": surface.line_count,
                "suggested_action": "move-to-reference",
            }));
        }
        if let Some(skill) = skill {
            let shared_terms = shared_guidance_terms(&body, &skill.body);
            if !shared_terms.is_empty() {
                findings.push(json!({
                    "id": "duplicate_guidance",
                    "severity": "warning",
                    "instruction_id": surface.instruction_id,
                    "path": surface.path,
                    "scope": surface.scope,
                    "skill": skill.name,
                    "skill_path": skill.path,
                    "terms": shared_terms,
                    "suggested_action": "review-conflict",
                }));
            }
            let conflicts = conflicting_terms(&body, &skill.body);
            if !conflicts.is_empty() {
                findings.push(json!({
                    "id": "conflicting_guidance",
                    "severity": "error",
                    "instruction_id": surface.instruction_id,
                    "path": surface.path,
                    "scope": surface.scope,
                    "skill": skill.name,
                    "terms": conflicts,
                    "suggested_action": "review-conflict",
                }));
            }
        }
    }

    Ok(findings)
}

pub(super) fn read_text_lossy(path: &Path) -> std::result::Result<ContentSignals, CommandFailure> {
    let bytes = fs::read(path).map_err(map_io)?;
    let size_bytes = bytes.len() as u64;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let line_count = body.lines().count();
    let lower = body.to_ascii_lowercase();
    let mut signals = BTreeSet::new();
    if lower.contains("use when") || lower.contains("trigger") {
        signals.insert("trigger_phrase".to_string());
    }
    if lower.contains("workflow") || lower.contains("steps") || lower.contains("runbook") {
        signals.insert("workflow_steps".to_string());
    }
    if has_any(
        &lower,
        &["cargo ", "npm ", "pytest", "go test", "make ", "gh "],
    ) {
        signals.insert("command_reference".to_string());
    }
    if has_any(
        &lower,
        &[
            "security", "secret", "do not", "never", "must not", "policy",
        ],
    ) {
        signals.insert("policy_or_safety".to_string());
    }
    if has_any(&lower, &["test", "lint", "ci", "build"]) {
        signals.insert("test_or_ci".to_string());
    }
    if line_count > 80 {
        signals.insert("large_instruction".to_string());
    }

    let contains_skill_like_workflow = signals.iter().any(|signal| {
        matches!(
            signal.as_str(),
            "trigger_phrase" | "workflow_steps" | "command_reference" | "test_or_ci"
        )
    });
    let suggested_action = if line_count > 120 {
        "move-to-reference"
    } else if contains_skill_like_workflow {
        "extract-skill"
    } else {
        "keep-instruction"
    };
    Ok(ContentSignals {
        signals: signals.into_iter().collect(),
        contains_skill_like_workflow,
        suggested_action: suggested_action.to_string(),
        warnings: Vec::new(),
        line_count,
        size_bytes,
        path_patterns: apply_to_patterns(&body),
    })
}

pub(super) fn read_text_body(path: &Path) -> std::result::Result<String, CommandFailure> {
    let bytes = fs::read(path).map_err(map_io)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn load_skill_for_doctor(
    app: &App,
    skill: &str,
) -> std::result::Result<DoctorSkill, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let skill_dir = app.ctx.skill_path(skill);
    let portable = skill_dir.join("SKILL.md");
    let legacy = skill_dir.join("skill.md");
    let path = if portable.is_file() {
        portable
    } else if legacy.is_file() {
        legacy
    } else {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    };
    let body = fs::read_to_string(&path).map_err(map_io)?;
    Ok(DoctorSkill {
        name: skill.to_string(),
        path: path.display().to_string(),
        body,
    })
}

pub(super) fn proposed_skill_name(surface: &InstructionSurface) -> String {
    Path::new(&surface.path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(portable_skill_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| portable_skill_name(&surface.instruction_id))
}

pub(super) fn validate_migration_name(
    target: InstructionMigrationTarget,
    name: Option<&str>,
) -> std::result::Result<(), CommandFailure> {
    let Some(name) = name else {
        return Ok(());
    };
    match target {
        InstructionMigrationTarget::Skill => {
            if let Some(message) = portable_name_error(name) {
                return Err(CommandFailure::new(ErrorCode::ArgInvalid, message));
            }
            Ok(())
        }
        InstructionMigrationTarget::Reference | InstructionMigrationTarget::KeepInstruction => {
            validate_skill_name(name).map_err(map_arg)
        }
    }
}

pub(super) fn migration_plan(
    surface: &InstructionSurface,
    target: InstructionMigrationTarget,
    proposed_name: &str,
) -> Value {
    let plan_id = format!("plan_{}", surface.instruction_id);
    match target {
        InstructionMigrationTarget::Skill => json!({
            "plan_id": plan_id,
            "target": "skill",
            "action": "extract-skill",
            "review_required": true,
            "writes": [],
            "would_write": [{
                "path": format!("skills/{proposed_name}/SKILL.md"),
                "purpose": "draft portable skill entrypoint"
            }],
            "notes": [
                "dry-run only; no instruction file or registry state was modified",
                "generated draft must pass skill lint before projection"
            ]
        }),
        InstructionMigrationTarget::Reference => json!({
            "plan_id": plan_id,
            "target": "reference",
            "action": "move-to-reference",
            "review_required": true,
            "writes": [],
            "would_write": [{
                "path": format!(
                    "skills/{proposed_name}/references/{}.md",
                    proposed_skill_name(surface)
                ),
                "purpose": "candidate reference split for repeated background guidance"
            }],
            "notes": [
                "dry-run only; source instruction edits are intentionally deferred",
                "human review must decide whether the always-on instruction keeps a short pointer"
            ]
        }),
        InstructionMigrationTarget::KeepInstruction => json!({
            "plan_id": plan_id,
            "target": "keep-instruction",
            "action": "keep-instruction",
            "review_required": false,
            "writes": [],
            "would_write": [],
            "notes": [
                "instruction remains native always-on or project-scoped guidance",
                "no file changes are needed"
            ]
        }),
    }
}

fn has_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn shared_guidance_terms(instruction: &str, skill: &str) -> Vec<String> {
    let instruction = instruction.to_ascii_lowercase();
    let skill = skill.to_ascii_lowercase();
    guidance_terms()
        .into_iter()
        .filter(|term| instruction.contains(term) && skill.contains(term))
        .map(str::to_string)
        .collect()
}

fn conflicting_terms(instruction: &str, skill: &str) -> Vec<String> {
    let instruction = instruction.to_ascii_lowercase();
    let skill = skill.to_ascii_lowercase();
    guidance_terms()
        .into_iter()
        .filter(|term| {
            (skill.contains(term) && has_negative_phrase(&instruction, term))
                || (instruction.contains(term) && has_negative_phrase(&skill, term))
        })
        .map(str::to_string)
        .collect()
}

fn has_negative_phrase(body: &str, term: &str) -> bool {
    [
        format!("do not {term}"),
        format!("never {term}"),
        format!("must not {term}"),
        format!("skip {term}"),
    ]
    .iter()
    .any(|phrase| body.contains(phrase))
}

fn guidance_terms() -> Vec<&'static str> {
    vec![
        "auth",
        "build",
        "catalog",
        "ci",
        "install",
        "lint",
        "merge",
        "migration",
        "provider",
        "release",
        "review",
        "rollback",
        "secret",
        "security",
        "skill",
        "test",
        "workflow",
    ]
}

fn apply_to_patterns(body: &str) -> Vec<String> {
    let mut in_frontmatter = false;
    let mut saw_frontmatter_open = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if !saw_frontmatter_open {
                saw_frontmatter_open = true;
                in_frontmatter = true;
                continue;
            }
            break;
        }
        if !in_frontmatter {
            break;
        }
        let Some((key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        if key.trim() != "applyTo" {
            continue;
        }
        return raw_value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .split(',')
            .map(str::trim)
            .filter(|pattern| !pattern.is_empty())
            .map(str::to_string)
            .collect();
    }
    Vec::new()
}

fn portable_skill_name(raw: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in raw.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('-');
            last_was_sep = true;
        }
    }
    let normalized = out.trim_matches('-').replace("--", "-");
    if normalized.is_empty() {
        "instruction-skill".to_string()
    } else {
        normalized
    }
}

fn portable_name_error(name: &str) -> Option<String> {
    if !(1..=64).contains(&name.len()) {
        return Some("skill name must be 1-64 characters".to_string());
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Some("skill name must not start or end with '-'".to_string());
    }
    if name.contains("--") {
        return Some("skill name must not contain consecutive '-'".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Some("skill name must use lowercase letters, digits, and hyphens only".to_string());
    }
    None
}
