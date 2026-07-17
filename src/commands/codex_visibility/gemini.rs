use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::agent_adapters::AgentAdapter;
use crate::commands::skill_lint::frontmatter::parse_skill_frontmatter;
use crate::gemini_cli::load_config;

use super::{CodexVisibilityCheck, check, normalize_existing_or_raw};

pub(super) fn target_requires_workspace_trust(adapter: &AgentAdapter, target: &Path) -> bool {
    !adapter
        .default_skill_dirs
        .iter()
        .any(|root| normalize_existing_or_raw(root) == normalize_existing_or_raw(target))
}

pub(super) fn add_gemini_config_checks(
    _root: &Path,
    skill: &str,
    workspace: Option<&Path>,
    project_scope_selected: bool,
    checks: &mut Vec<CodexVisibilityCheck>,
) -> bool {
    let state = match load_config(skill, workspace) {
        Ok(state) => state,
        Err(_) => {
            push_requirement(checks, "gemini-cli_config_valid", false);
            return true;
        }
    };
    push_requirement(checks, "gemini-cli_config_valid", true);
    push_requirement(
        checks,
        "gemini-cli_skills_enabled",
        state.settings.skills_enabled,
    );
    let skill_enabled = !state.settings.skill_disabled;
    push_requirement(checks, "gemini-cli_skill_not_disabled", skill_enabled);
    push_requirement(checks, "gemini-cli_admin_policy_observable", false);

    let workspace_allowed = !project_scope_selected || state.workspace_trusted == Some(true);
    if project_scope_selected {
        checks.push(check(
            "gemini-cli_workspace_trusted",
            workspace_allowed,
            "error",
            "Gemini CLI project skills require a trusted workspace",
            json!({"trusted": state.workspace_trusted}),
            Some("run /permissions trust for this workspace".to_string()),
        ));
    }

    true
}

fn push_requirement(checks: &mut Vec<CodexVisibilityCheck>, id: &str, ok: bool) {
    let (message, action) = match id {
        "gemini-cli_config_valid" => (
            "Gemini CLI settings and trust files must be valid",
            "repair Gemini CLI settings or trustedFolders.json",
        ),
        "gemini-cli_skills_enabled" => (
            "Gemini CLI skills must be enabled",
            "enable skills.enabled, then run /skills reload",
        ),
        "gemini-cli_skill_not_disabled" => (
            "skill must not be disabled in Gemini CLI",
            "run /skills enable for this skill, then /skills reload",
        ),
        _ => (
            "Gemini CLI remote admin policy must be confirmed",
            "confirm the effective policy with /skills list",
        ),
    };
    checks.push(check(
        id,
        ok,
        "error",
        message,
        Value::Null,
        Some(action.to_string()),
    ));
}

pub(super) fn add_frontmatter_check(
    entrypoint: &Path,
    skill: &str,
    target_id: &str,
    checks: &mut Vec<CodexVisibilityCheck>,
) -> bool {
    let strict_valid = parse_skill_frontmatter(entrypoint)
        .ok()
        .is_some_and(|parsed| {
            parsed
                .frontmatter
                .name
                .as_deref()
                .is_some_and(|name| sanitize_name(name).eq_ignore_ascii_case(skill))
                && parsed
                    .frontmatter
                    .description
                    .as_deref()
                    .is_some_and(|description| !description.is_empty())
        });
    let valid = strict_valid || simple_frontmatter_valid(entrypoint, skill);
    checks.push(check(
        &format!("gemini-cli_frontmatter_valid:{target_id}"),
        valid,
        "error",
        "projected SKILL.md requires valid Gemini name and description frontmatter",
        Value::Null,
        Some("add valid name and description frontmatter to SKILL.md".to_string()),
    ));
    valid
}

fn simple_frontmatter_valid(entrypoint: &Path, skill: &str) -> bool {
    let Ok(raw) = fs::read_to_string(entrypoint) else {
        return false;
    };
    let mut lines = raw.lines();
    if lines.next().map(str::trim) != Some("---") {
        return false;
    }
    let mut name = None;
    let mut description = None::<String>;
    let mut closed = false;
    for raw_line in lines {
        let line = raw_line.trim();
        if line == "---" {
            closed = true;
            break;
        }
        if let Some(value) = line.strip_prefix("name:") {
            name = Some(sanitize_name(value.trim()));
        } else if let Some(value) = line.strip_prefix("description:") {
            description = Some(value.trim().to_string());
        } else if raw_line.starts_with([' ', '\t'])
            && let Some(description) = description.as_mut()
            && !line.is_empty()
        {
            if !description.is_empty() {
                description.push(' ');
            }
            description.push_str(line);
        }
    }
    closed && name.is_some_and(|name| name.eq_ignore_ascii_case(skill)) && description.is_some()
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            ':' | '\\' | '/' | '<' | '>' | '*' | '?' | '"' | '|' => '-',
            _ => character,
        })
        .collect()
}
