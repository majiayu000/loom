use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::frontmatter::FrontmatterSchemaIssue;
use super::{
    SkillLintConfig, SkillLintFinding, SkillLintFrontmatter, SkillLintMode,
    SkillLintProgressiveDisclosure, SkillLintResources, SkillLintSection, SkillLintSections,
    lint_or_warn, push_finding,
};

pub(crate) fn collect_resources(skill_path: &Path) -> SkillLintResources {
    SkillLintResources {
        scripts: count_files(skill_path.join("scripts")),
        references: count_files(skill_path.join("references")),
        assets: count_files(skill_path.join("assets")),
    }
}

fn count_files(path: PathBuf) -> usize {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| entry.path().is_file())
        .count()
}

pub(crate) fn inspect_progressive_disclosure(entrypoint: &Path) -> SkillLintProgressiveDisclosure {
    let raw = fs::read_to_string(entrypoint).unwrap_or_default();
    SkillLintProgressiveDisclosure {
        main_line_count: raw.lines().count(),
        main_token_estimate: raw.split_whitespace().count() * 4 / 3,
    }
}

pub(crate) fn push_schema_issue(
    findings: &mut Vec<SkillLintFinding>,
    mode: SkillLintMode,
    issue: FrontmatterSchemaIssue,
) {
    lint_or_warn(
        findings,
        mode,
        &issue.id,
        &issue.message,
        &issue.suggested_action,
        false,
        issue.details,
    );
}

pub(crate) fn run_agent_checks(
    config: &SkillLintConfig,
    frontmatter: &SkillLintFrontmatter,
    findings: &mut Vec<SkillLintFinding>,
) {
    let Some(agent) = config.agent.as_deref() else {
        return;
    };
    match agent {
        "codex" => {
            let unsupported = frontmatter
                .agent_fields
                .iter()
                .filter(|field| claude_only_field(field))
                .cloned()
                .collect::<Vec<_>>();
            if !unsupported.is_empty() {
                push_finding(
                    findings,
                    "agent_codex_unsupported_field",
                    "warning",
                    "frontmatter declares fields that Codex may not honor",
                    "move Claude-specific behavior behind a Claude compatibility note or add Codex fallback instructions",
                    false,
                    json!({ "agent": "codex", "fields": unsupported }),
                );
            }
        }
        "claude" => {}
        other => push_finding(
            findings,
            "agent_unknown",
            "warning",
            "skill lint does not have target-agent rules for this agent",
            "use --agent codex or --agent claude for built-in compatibility checks",
            false,
            json!({ "agent": other }),
        ),
    }
}

fn claude_only_field(field: &str) -> bool {
    matches!(
        field,
        "allowed-tools"
            | "disallowed-tools"
            | "disable-model-invocation"
            | "user-invocable"
            | "argument-hint"
            | "paths"
            | "model"
            | "effort"
            | "context"
            | "agent"
    )
}

pub(crate) fn run_quality_checks(
    skill_path: &Path,
    frontmatter: &SkillLintFrontmatter,
    resources: &SkillLintResources,
    progressive: &SkillLintProgressiveDisclosure,
    findings: &mut Vec<SkillLintFinding>,
) {
    if let Some(description) = frontmatter.description.as_deref() {
        let lower = description.to_ascii_lowercase();
        let concrete_trigger = [
            "when", "use", "for", "trigger", "asks", "needs", "debug", "review",
        ]
        .iter()
        .any(|needle| lower.contains(needle));
        if !concrete_trigger || description.split_whitespace().count() < 8 {
            push_quality_finding(
                findings,
                "quality_description_vague",
                "frontmatter description may be too vague to trigger reliably",
                "include concrete task words and when-to-use context",
                json!({ "description": description }),
            );
        }
        if lower.contains("always use") {
            push_quality_finding(
                findings,
                "quality_description_overbroad",
                "frontmatter description uses an over-broad trigger",
                "replace always-on language with specific task conditions",
                json!({ "description": description }),
            );
        }
    }

    if progressive.main_line_count > 400 {
        push_quality_finding(
            findings,
            "quality_skill_md_large",
            "SKILL.md is large enough to risk context bloat",
            "move background material into references/ and keep SKILL.md task-focused",
            json!({ "main_line_count": progressive.main_line_count }),
        );
    }

    if !skill_path.join("evals/triggers.jsonl").is_file()
        && !skill_path.join("evals/tasks.jsonl").is_file()
    {
        push_quality_finding(
            findings,
            "quality_evals_missing",
            "non-trivial skills should include trigger or task eval fixtures",
            "add evals/triggers.jsonl or evals/tasks.jsonl",
            json!({ "accepted": ["evals/triggers.jsonl", "evals/tasks.jsonl"] }),
        );
    }

    if resources.scripts > 0 {
        for script in script_files_without_shebang(skill_path) {
            push_quality_finding(
                findings,
                "quality_script_entrypoint_unclear",
                "script file lacks a shebang or nearby usage documentation",
                "add a shebang, README, or explicit usage note for scripts",
                json!({ "path": script }),
            );
        }
    }
}

fn script_files_without_shebang(skill_path: &Path) -> Vec<String> {
    let scripts = skill_path.join("scripts");
    let has_usage_doc = scripts.join("README.md").is_file() || scripts.join("USAGE.md").is_file();
    if has_usage_doc {
        return Vec::new();
    }
    fs::read_dir(scripts)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() || path.extension().is_some_and(|ext| ext == "md") {
                return None;
            }
            let raw = fs::read_to_string(&path).unwrap_or_default();
            (!raw.starts_with("#!")).then(|| path.display().to_string())
        })
        .collect()
}

fn push_quality_finding(
    findings: &mut Vec<SkillLintFinding>,
    id: &str,
    message: &str,
    suggested_action: &str,
    details: Value,
) {
    push_finding(
        findings,
        id,
        "warning",
        message,
        suggested_action,
        false,
        details,
    );
}

pub(crate) fn build_sections(
    findings: &[SkillLintFinding],
    agent: Option<&str>,
    resources: SkillLintResources,
    progressive_disclosure: SkillLintProgressiveDisclosure,
) -> SkillLintSections {
    let portable_spec = section_from_findings(findings.iter().filter(|finding| {
        !finding.id.starts_with("agent_") && !finding.id.starts_with("quality_")
    }));
    let quality = section_from_findings(
        findings
            .iter()
            .filter(|finding| finding.id.starts_with("quality_")),
    );
    let mut agent_compatibility = BTreeMap::new();
    if let Some(agent) = agent {
        let section = section_from_findings(findings.iter().filter(|finding| {
            finding.id.starts_with("agent_") && finding.details["agent"].as_str() == Some(agent)
        }));
        agent_compatibility.insert(agent.to_string(), section);
    }
    SkillLintSections {
        portable_spec,
        agent_compatibility,
        quality,
        resources,
        progressive_disclosure,
    }
}

fn section_from_findings<'a>(
    findings: impl Iterator<Item = &'a SkillLintFinding>,
) -> SkillLintSection {
    let findings = findings.collect::<Vec<_>>();
    let status = if findings.iter().any(|finding| finding.severity == "error") {
        "error"
    } else if findings.iter().any(|finding| finding.severity == "warning") {
        "warning"
    } else {
        "pass"
    };
    SkillLintSection {
        status: status.to_string(),
        findings: findings
            .into_iter()
            .map(|finding| finding.id.clone())
            .collect(),
    }
}
