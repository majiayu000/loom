use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::agent_adapters::AgentAdapter;
use crate::commands::skill_lint::frontmatter::parse_skill_frontmatter;
use crate::state::home_dir;

use super::{CodexVisibilityCheck, check, normalize_existing_or_raw};

const INVALID_CONFIG: &str = "Gemini CLI settings or trust configuration is invalid";

#[derive(Debug)]
struct GeminiSettings {
    skills_enabled: bool,
    disabled_skills: Vec<String>,
    folder_trust_enabled: bool,
}

impl Default for GeminiSettings {
    fn default() -> Self {
        Self {
            skills_enabled: true,
            disabled_skills: Vec::new(),
            folder_trust_enabled: true,
        }
    }
}

struct GeminiConfigState {
    settings: GeminiSettings,
    workspace_trusted: Option<bool>,
}

pub(super) fn target_requires_workspace_trust(adapter: &AgentAdapter, target: &Path) -> bool {
    !adapter
        .default_skill_dirs
        .iter()
        .any(|root| normalize_existing_or_raw(root) == normalize_existing_or_raw(target))
}

pub(super) fn add_gemini_config_checks(
    skill: &str,
    workspace: Option<&Path>,
    project_scope_selected: bool,
    checks: &mut Vec<CodexVisibilityCheck>,
) -> bool {
    let state = match load_config(workspace) {
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
    let skill_enabled = !state
        .settings
        .disabled_skills
        .iter()
        .any(|disabled| disabled.eq_ignore_ascii_case(skill));
    push_requirement(checks, "gemini-cli_skill_not_disabled", skill_enabled);
    push_requirement(checks, "gemini-cli_admin_policy_observable", false);

    let workspace_allowed = !project_scope_selected || state.workspace_trusted == Some(true);
    if project_scope_selected {
        checks.push(check(
            "gemini-cli_workspace_trusted",
            workspace_allowed,
            "error",
            "gemini-cli_workspace_trusted",
            json!({"trusted": state.workspace_trusted}),
            None,
        ));
    }

    true
}

fn push_requirement(checks: &mut Vec<CodexVisibilityCheck>, id: &str, ok: bool) {
    checks.push(check(id, ok, "error", id, Value::Null, None));
}

pub(super) fn add_frontmatter_check(
    entrypoint: &Path,
    skill: &str,
    target_id: &str,
    checks: &mut Vec<CodexVisibilityCheck>,
) -> bool {
    let valid = parse_skill_frontmatter(entrypoint).is_ok_and(|parsed| {
        parsed.schema_issues.is_empty()
            && parsed.frontmatter.name.as_deref() == Some(skill)
            && parsed
                .frontmatter
                .description
                .as_deref()
                .is_some_and(|description| !description.is_empty())
    });
    checks.push(check(
        &format!("gemini-cli_frontmatter_valid:{target_id}"),
        valid,
        "error",
        "gemini-cli_frontmatter_valid",
        Value::Null,
        None,
    ));
    valid
}

fn load_config(workspace: Option<&Path>) -> Result<GeminiConfigState, &'static str> {
    let home = gemini_cli_home().ok_or(INVALID_CONFIG)?;
    let defaults = system_path("GEMINI_CLI_SYSTEM_DEFAULTS_PATH", "system-defaults.json");
    let user = home.join(".gemini/settings.json");
    let system = system_path("GEMINI_CLI_SYSTEM_SETTINGS_PATH", "settings.json");

    let mut settings = GeminiSettings::default();
    for path in [defaults.as_deref(), Some(user.as_path()), system.as_deref()] {
        apply_layer(&mut settings, path)?;
    }
    let workspace_trusted = match workspace {
        Some(path) => workspace_trust(path, &home, settings.folder_trust_enabled)?,
        None => None,
    };
    if workspace_trusted == Some(true)
        && let Some(workspace) = workspace
    {
        apply_layer(
            &mut settings,
            Some(&workspace.join(".gemini/settings.json")),
        )?;
        apply_layer(&mut settings, system.as_deref())?;
    }
    Ok(GeminiConfigState {
        settings,
        workspace_trusted,
    })
}

fn apply_layer(settings: &mut GeminiSettings, path: Option<&Path>) -> Result<(), &'static str> {
    let Some(path) = path.filter(|path| path.exists()) else {
        return Ok(());
    };
    let raw = fs::read_to_string(path).map_err(|_| INVALID_CONFIG)?;
    let value = parse_json(&raw)?;
    let root = value.as_object().ok_or(INVALID_CONFIG)?;
    if let Some(value) = root.get("skills") {
        let skills = value.as_object().ok_or(INVALID_CONFIG)?;
        if let Some(value) = skills.get("enabled") {
            settings.skills_enabled = value.as_bool().ok_or(INVALID_CONFIG)?;
        }
        if let Some(value) = skills.get("disabled") {
            for value in value.as_array().ok_or(INVALID_CONFIG)? {
                settings
                    .disabled_skills
                    .push(value.as_str().ok_or(INVALID_CONFIG)?.to_string());
            }
        }
    }
    if let Some(value) = root.get("security") {
        let security = value.as_object().ok_or(INVALID_CONFIG)?;
        if let Some(value) = security.get("folderTrust") {
            let trust = value.as_object().ok_or(INVALID_CONFIG)?;
            if let Some(value) = trust.get("enabled") {
                settings.folder_trust_enabled = value.as_bool().ok_or(INVALID_CONFIG)?;
            }
        }
    }
    Ok(())
}

fn workspace_trust(
    workspace: &Path,
    home: &Path,
    folder_trust_enabled: bool,
) -> Result<Option<bool>, &'static str> {
    if env::var("GEMINI_CLI_TRUST_WORKSPACE").as_deref() == Ok("true") || !folder_trust_enabled {
        return Ok(Some(true));
    }
    let path = env::var_os("GEMINI_CLI_TRUSTED_FOLDERS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".gemini/trustedFolders.json"));
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(|_| INVALID_CONFIG)?;
    let value = parse_json(&raw)?;
    let rules = value.as_object().ok_or(INVALID_CONFIG)?;
    let workspace = normalize_existing_or_raw(workspace);
    let mut longest = None;
    for (raw_path, value) in rules {
        let level = value.as_str().ok_or(INVALID_CONFIG)?;
        let rule = PathBuf::from(raw_path);
        let effective = match level {
            "TRUST_FOLDER" | "DO_NOT_TRUST" => rule,
            "TRUST_PARENT" => rule.parent().map(Path::to_path_buf).unwrap_or(rule),
            _ => return Err(INVALID_CONFIG),
        };
        if workspace.starts_with(normalize_existing_or_raw(&effective)) {
            let candidate = (raw_path.len(), level != "DO_NOT_TRUST");
            if longest.is_none_or(|current: (usize, bool)| candidate.0 > current.0) {
                longest = Some(candidate);
            }
        }
    }
    Ok(longest.map(|(_, trusted)| trusted))
}

fn parse_json(raw: &str) -> Result<Value, &'static str> {
    serde_json::from_str(&strip_comments(raw)).map_err(|_| INVALID_CONFIG)
}

fn strip_comments(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                string = false;
            }
        } else if ch == '"' {
            string = true;
            output.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            if chars.by_ref().any(|comment| comment == '\n') {
                output.push('\n');
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut previous = '\0';
            for comment in chars.by_ref() {
                if comment == '\n' {
                    output.push('\n');
                }
                if previous == '*' && comment == '/' {
                    break;
                }
                previous = comment;
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn gemini_cli_home() -> Option<PathBuf> {
    env::var_os("GEMINI_CLI_HOME")
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .or_else(home_dir)
}

fn system_path(env_var: &str, name: &str) -> Option<PathBuf> {
    env::var_os(env_var)
        .map(PathBuf::from)
        .or_else(|| system_dir().map(|path| path.join(name)))
}

#[cfg(target_os = "linux")]
fn system_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/etc/gemini-cli"))
}

#[cfg(target_os = "macos")]
fn system_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/Library/Application Support/GeminiCli"))
}

#[cfg(target_os = "windows")]
fn system_dir() -> Option<PathBuf> {
    env::var_os("ProgramData").map(|path| PathBuf::from(path).join("gemini-cli"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn system_dir() -> Option<PathBuf> {
    None
}
