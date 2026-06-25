use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::SkillLintArgs;
use crate::envelope::Meta;
use crate::types::ErrorCode;

use super::helpers::{map_arg, validate_skill_name};
use super::{App, CommandFailure};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkillLintMode {
    Strict,
    Compat,
    Fix,
}

impl SkillLintMode {
    fn from_args(args: &SkillLintArgs) -> Self {
        if args.compat {
            Self::Compat
        } else if args.fix {
            Self::Fix
        } else {
            Self::Strict
        }
    }

    fn mode_label(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Compat => "compat",
            Self::Fix => "fix",
        }
    }

    fn strict_errors(self) -> bool {
        matches!(self, Self::Strict)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillLintReport {
    pub skill: String,
    pub mode: SkillLintMode,
    pub valid: bool,
    pub compatible: bool,
    pub path: String,
    pub entrypoint: SkillLintEntrypoint,
    pub frontmatter: SkillLintFrontmatter,
    pub summary: SkillLintSummary,
    pub findings: Vec<SkillLintFinding>,
    pub fix_plan: Vec<SkillLintFix>,
}

impl SkillLintReport {
    pub(crate) fn description(&self) -> Option<&str> {
        self.frontmatter.description.as_deref()
    }

    pub(crate) fn entrypoint_path(&self) -> Option<&str> {
        self.entrypoint.path.as_deref()
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillLintEntrypoint {
    pub status: String,
    pub file_name: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SkillLintFrontmatter {
    pub present: bool,
    pub parsed: bool,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillLintSummary {
    pub error_count: usize,
    pub warning_count: usize,
    pub fixable_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillLintFinding {
    pub id: String,
    pub severity: String,
    pub message: String,
    pub suggested_action: String,
    pub fixable: bool,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillLintFix {
    pub id: String,
    pub action: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub applied: bool,
}

struct EntrypointProbe {
    status: &'static str,
    file_name: Option<&'static str>,
    path: Option<PathBuf>,
}

impl App {
    pub fn cmd_skill_lint(
        &self,
        args: &SkillLintArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let skill_path = self.ctx.skill_path(&args.skill);
        if !skill_path.is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let mode = SkillLintMode::from_args(args);
        let report = lint_skill_source(&skill_path, &args.skill, mode);
        if mode.strict_errors() && report.summary.error_count > 0 {
            let mut failure = CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!(
                    "skill '{}' failed {} lint with {} error(s)",
                    args.skill,
                    mode.mode_label(),
                    report.summary.error_count
                ),
            );
            failure.details = json!({ "report": report });
            return Err(failure);
        }

        Ok((json!(report), Meta::default()))
    }
}

pub(crate) fn lint_skill_source(
    skill_path: &Path,
    expected_name: &str,
    mode: SkillLintMode,
) -> SkillLintReport {
    let probe = find_entrypoint(skill_path);
    let mut findings = Vec::new();
    let mut fix_plan = Vec::new();
    let mut frontmatter = SkillLintFrontmatter::default();

    if !skill_path.is_dir() {
        push_finding(
            &mut findings,
            "source_directory_missing",
            "error",
            "skill source directory is missing",
            "restore the source skill or remove stale registry references",
            false,
            json!({ "path": skill_path.display().to_string() }),
        );
    }

    match probe.status {
        "missing" => push_finding(
            &mut findings,
            "entrypoint_missing",
            "error",
            "skill entrypoint is missing",
            "add SKILL.md to the skill directory",
            false,
            json!({ "accepted": ["SKILL.md", "skill.md"] }),
        ),
        "legacy" => {
            let severity = if mode.strict_errors() {
                "error"
            } else {
                "warning"
            };
            push_finding(
                &mut findings,
                "entrypoint_case",
                severity,
                "legacy lowercase skill.md entrypoint is not portable",
                "rename skill.md to SKILL.md",
                true,
                json!({ "found": "skill.md", "required": "SKILL.md" }),
            );
            fix_plan.push(SkillLintFix {
                id: "rename_entrypoint".to_string(),
                action: "rename skill.md to SKILL.md".to_string(),
                from: Some(skill_path.join("skill.md").display().to_string()),
                to: Some(skill_path.join("SKILL.md").display().to_string()),
                applied: false,
            });
        }
        _ => {}
    }

    if let Some(entrypoint) = probe.path.as_ref() {
        match parse_skill_frontmatter(entrypoint) {
            Ok(parsed) => {
                frontmatter = parsed;
                validate_frontmatter(expected_name, &frontmatter, mode, &mut findings);
            }
            Err(message) => {
                let severity = if mode.strict_errors() {
                    "error"
                } else {
                    "warning"
                };
                push_finding(
                    &mut findings,
                    "frontmatter_yaml_invalid",
                    severity,
                    "skill frontmatter YAML did not parse",
                    "fix YAML frontmatter between the opening and closing --- markers",
                    false,
                    json!({ "error": message }),
                );
            }
        }
    }

    let error_count = findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    let warning_count = findings
        .iter()
        .filter(|finding| finding.severity == "warning")
        .count();
    let fixable_count = findings.iter().filter(|finding| finding.fixable).count();
    let compatible = probe.status != "missing" && skill_path.is_dir();

    SkillLintReport {
        skill: expected_name.to_string(),
        mode,
        valid: error_count == 0,
        compatible,
        path: skill_path.display().to_string(),
        entrypoint: SkillLintEntrypoint {
            status: probe.status.to_string(),
            file_name: probe.file_name.map(str::to_string),
            path: probe.path.map(|path| path.display().to_string()),
        },
        frontmatter,
        summary: SkillLintSummary {
            error_count,
            warning_count,
            fixable_count,
        },
        findings,
        fix_plan,
    }
}

fn find_entrypoint(skill_path: &Path) -> EntrypointProbe {
    let mut portable = None;
    let mut legacy = None;
    if let Ok(entries) = fs::read_dir(skill_path) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            if file_name == "SKILL.md" && entry.path().is_file() {
                portable = Some(entry.path());
            } else if file_name == "skill.md" && entry.path().is_file() {
                legacy = Some(entry.path());
            }
        }
    }
    if let Some(path) = portable {
        return EntrypointProbe {
            status: "portable",
            file_name: Some("SKILL.md"),
            path: Some(path),
        };
    }
    if let Some(path) = legacy {
        return EntrypointProbe {
            status: "legacy",
            file_name: Some("skill.md"),
            path: Some(path),
        };
    }
    EntrypointProbe {
        status: "missing",
        file_name: None,
        path: None,
    }
}

fn parse_skill_frontmatter(entrypoint: &Path) -> Result<SkillLintFrontmatter, String> {
    let raw = fs::read_to_string(entrypoint).map_err(|err| err.to_string())?;
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Ok(SkillLintFrontmatter::default());
    }

    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim() == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("frontmatter is missing a closing --- marker".to_string());
    }

    let metadata = parse_frontmatter_metadata(&yaml)?;

    Ok(SkillLintFrontmatter {
        present: true,
        parsed: true,
        name: metadata.name,
        description: metadata.description,
    })
}

#[derive(Default)]
struct FrontmatterMetadata {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter_metadata(raw: &str) -> Result<FrontmatterMetadata, String> {
    let mut metadata = FrontmatterMetadata::default();

    for (index, line) in raw.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.chars().next().is_some_and(char::is_whitespace) {
            return Err(format!(
                "frontmatter line {line_no} uses nested or continued YAML; use top-level string fields"
            ));
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            return Err(format!(
                "frontmatter line {line_no} is not a key/value mapping entry"
            ));
        };
        let key = key.trim();
        validate_frontmatter_key(key, line_no)?;

        let parsed = parse_frontmatter_string(value.trim(), key, line_no)?;
        match key {
            "name" => metadata.name = parsed,
            "description" => metadata.description = parsed,
            _ => {}
        }
    }

    Ok(metadata)
}

fn validate_frontmatter_key(key: &str, line_no: usize) -> Result<(), String> {
    if key.is_empty() {
        return Err(format!("frontmatter line {line_no} has an empty key"));
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(format!("frontmatter line {line_no} has an unsupported key"));
    }
    Ok(())
}

fn parse_frontmatter_string(
    raw: &str,
    key: &str,
    line_no: usize,
) -> Result<Option<String>, String> {
    let raw = raw.trim();
    let value = if raw.starts_with('"') || raw.starts_with('\'') {
        raw
    } else {
        strip_plain_comment(raw).trim()
    };
    if value.is_empty() {
        return Ok(None);
    }
    if value.starts_with('[')
        || value.starts_with('{')
        || value.starts_with('|')
        || value.starts_with('>')
    {
        return Err(format!(
            "frontmatter field '{key}' on line {line_no} must be a string scalar"
        ));
    }

    let parsed = if value.starts_with('"') {
        parse_double_quoted_scalar(value, key, line_no)?
    } else if value.starts_with('\'') {
        parse_single_quoted_scalar(value, key, line_no)?
    } else {
        value.to_string()
    };

    let trimmed = parsed.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn strip_plain_comment(raw: &str) -> &str {
    let mut previous_was_space = true;
    for (index, ch) in raw.char_indices() {
        if ch == '#' && previous_was_space {
            return raw[..index].trim_end();
        }
        previous_was_space = ch.is_whitespace();
    }
    raw
}

fn parse_single_quoted_scalar(value: &str, key: &str, line_no: usize) -> Result<String, String> {
    if !value.ends_with('\'') || value.len() < 2 {
        return Err(format!(
            "frontmatter field '{key}' on line {line_no} has an unterminated quoted string"
        ));
    }
    Ok(value[1..value.len() - 1].replace("''", "'"))
}

fn parse_double_quoted_scalar(value: &str, key: &str, line_no: usize) -> Result<String, String> {
    if !value.ends_with('"') || value.len() < 2 {
        return Err(format!(
            "frontmatter field '{key}' on line {line_no} has an unterminated quoted string"
        ));
    }

    let mut parsed = String::new();
    let mut escaped = false;
    for ch in value[1..value.len() - 1].chars() {
        if escaped {
            let translated = match ch {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => {
                    return Err(format!(
                        "frontmatter field '{key}' on line {line_no} has unsupported escape '\\{other}'"
                    ));
                }
            };
            parsed.push(translated);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            parsed.push(ch);
        }
    }
    if escaped {
        return Err(format!(
            "frontmatter field '{key}' on line {line_no} ends with an incomplete escape"
        ));
    }

    Ok(parsed)
}

fn validate_frontmatter(
    expected_name: &str,
    frontmatter: &SkillLintFrontmatter,
    mode: SkillLintMode,
    findings: &mut Vec<SkillLintFinding>,
) {
    if !frontmatter.present {
        lint_or_warn(
            findings,
            mode,
            "frontmatter_missing",
            "skill frontmatter is missing",
            "add YAML frontmatter with name and description",
            false,
            json!({}),
        );
        return;
    }

    match frontmatter.name.as_deref() {
        Some(name) => {
            if let Some(message) = portable_name_error(name) {
                lint_or_warn(
                    findings,
                    mode,
                    "name_invalid",
                    &message,
                    "use lowercase letters, digits, and single hyphens only",
                    false,
                    json!({ "name": name }),
                );
            }
            if name != expected_name {
                lint_or_warn(
                    findings,
                    mode,
                    "name_directory_mismatch",
                    "frontmatter name does not match the skill directory",
                    "rename the directory or update frontmatter name",
                    false,
                    json!({ "frontmatter_name": name, "directory_name": expected_name }),
                );
            }
        }
        None => lint_or_warn(
            findings,
            mode,
            "name_missing",
            "frontmatter name is missing",
            "add a name field matching the skill directory",
            false,
            json!({ "directory_name": expected_name }),
        ),
    }

    match frontmatter.description.as_deref() {
        Some(description) => {
            if description.split_whitespace().count() < 6 {
                lint_or_warn(
                    findings,
                    mode,
                    "description_too_short",
                    "frontmatter description is too short to guide an agent",
                    "describe what the skill does and when to use it",
                    false,
                    json!({ "description": description }),
                );
            }
            let lower = description.to_ascii_lowercase();
            if !["use", "when", "for", "trigger"]
                .iter()
                .any(|needle| lower.contains(needle))
            {
                lint_or_warn(
                    findings,
                    mode,
                    "description_missing_usage_context",
                    "frontmatter description does not explain when to use the skill",
                    "include trigger or usage context in the description",
                    false,
                    json!({ "description": description }),
                );
            }
        }
        None => lint_or_warn(
            findings,
            mode,
            "description_missing",
            "frontmatter description is missing",
            "add a description explaining what the skill does and when to use it",
            false,
            json!({}),
        ),
    }
}

fn portable_name_error(name: &str) -> Option<String> {
    if !(1..=64).contains(&name.len()) {
        return Some("frontmatter name must be 1-64 characters".to_string());
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Some("frontmatter name must not start or end with '-'".to_string());
    }
    if name.contains("--") {
        return Some("frontmatter name must not contain consecutive '-'".to_string());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Some(
            "frontmatter name must use lowercase letters, digits, and hyphens only".to_string(),
        );
    }
    None
}

fn lint_or_warn(
    findings: &mut Vec<SkillLintFinding>,
    mode: SkillLintMode,
    id: &str,
    message: &str,
    suggested_action: &str,
    fixable: bool,
    details: Value,
) {
    let severity = if mode.strict_errors() {
        "error"
    } else {
        "warning"
    };
    push_finding(
        findings,
        id,
        severity,
        message,
        suggested_action,
        fixable,
        details,
    );
}

fn push_finding(
    findings: &mut Vec<SkillLintFinding>,
    id: &str,
    severity: &str,
    message: &str,
    suggested_action: &str,
    fixable: bool,
    details: Value,
) {
    findings.push(SkillLintFinding {
        id: id.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
        suggested_action: suggested_action.to_string(),
        fixable,
        details,
    });
}
