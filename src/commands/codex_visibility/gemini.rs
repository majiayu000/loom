use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::agent_adapters::AgentAdapter;
use crate::state::home_dir;

use super::{CodexVisibilityCheck, check, normalize_existing_or_raw};

#[derive(Debug)]
struct GeminiSettings {
    skills_enabled: bool,
    disabled_skills: Vec<String>,
    admin_skills_enabled: bool,
    folder_trust_enabled: bool,
    loaded_paths: Vec<String>,
}

impl Default for GeminiSettings {
    fn default() -> Self {
        Self {
            skills_enabled: true,
            disabled_skills: Vec::new(),
            admin_skills_enabled: true,
            folder_trust_enabled: true,
            loaded_paths: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct GeminiConfigState {
    settings: GeminiSettings,
    workspace_trusted: Option<bool>,
    trust_source: Option<&'static str>,
    trusted_folders_path: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct WorkspaceTrust {
    trusted: Option<bool>,
    source: Option<&'static str>,
    path: Option<PathBuf>,
}

pub(super) fn target_requires_workspace_trust(adapter: &AgentAdapter, target_path: &Path) -> bool {
    !adapter
        .default_skill_dirs
        .iter()
        .any(|root| normalize_existing_or_raw(root) == normalize_existing_or_raw(target_path))
}

pub(super) fn add_gemini_config_checks(
    skill: &str,
    workspace: Option<&Path>,
    project_scope_selected: bool,
    checks: &mut Vec<CodexVisibilityCheck>,
) -> bool {
    let state = match load_gemini_config(workspace) {
        Ok(state) => state,
        Err(error) => {
            checks.push(check(
                "gemini-cli_config_valid",
                false,
                "error",
                "Gemini CLI settings or trust configuration is invalid",
                json!({"error": error}),
                Some(
                    "repair Gemini CLI settings or trustedFolders.json before checking visibility"
                        .to_string(),
                ),
            ));
            return true;
        }
    };

    checks.push(check(
        "gemini-cli_config_valid",
        true,
        "warning",
        "Gemini CLI visibility settings loaded",
        json!({"settings_paths": state.settings.loaded_paths}),
        None,
    ));
    checks.push(check(
        "gemini-cli_skills_enabled",
        state.settings.skills_enabled,
        "error",
        if state.settings.skills_enabled {
            "Gemini CLI skills are enabled"
        } else {
            "Gemini CLI skills are disabled by skills.enabled"
        },
        json!({"skills_enabled": state.settings.skills_enabled}),
        Some("enable skills.enabled in Gemini CLI settings, then run /skills reload".to_string()),
    ));
    let skill_enabled = !state
        .settings
        .disabled_skills
        .iter()
        .any(|disabled| disabled == skill);
    checks.push(check(
        "gemini-cli_skill_not_disabled",
        skill_enabled,
        "error",
        if skill_enabled {
            "skill is not disabled in Gemini CLI settings"
        } else {
            "skill is disabled in Gemini CLI settings"
        },
        json!({"skill": skill, "disabled_skills": state.settings.disabled_skills}),
        Some(format!(
            "run /skills enable {skill} in Gemini CLI, then run /skills reload"
        )),
    ));
    checks.push(check(
        "gemini-cli_admin_skills_enabled",
        state.settings.admin_skills_enabled,
        "error",
        if state.settings.admin_skills_enabled {
            "Gemini CLI admin policy allows skills"
        } else {
            "Gemini CLI admin policy disables skills"
        },
        json!({"admin_skills_enabled": state.settings.admin_skills_enabled}),
        Some("ask the Gemini CLI administrator to enable admin.skills.enabled".to_string()),
    ));

    let workspace_allowed = !project_scope_selected || state.workspace_trusted == Some(true);
    if project_scope_selected {
        checks.push(check(
            "gemini-cli_workspace_trusted",
            workspace_allowed,
            "error",
            if workspace_allowed {
                "Gemini CLI workspace is trusted for project skill discovery"
            } else {
                "Gemini CLI workspace trust is absent or denied; project skills are not loaded"
            },
            json!({
                "workspace": workspace,
                "folder_trust_enabled": state.settings.folder_trust_enabled,
                "workspace_trusted": state.workspace_trusted,
                "trust_source": state.trust_source,
                "trusted_folders_path": state.trusted_folders_path,
            }),
            Some("run /permissions trust in Gemini CLI for this workspace".to_string()),
        ));
    }

    !state.settings.skills_enabled
        || !skill_enabled
        || !state.settings.admin_skills_enabled
        || !workspace_allowed
}

fn load_gemini_config(workspace: Option<&Path>) -> Result<GeminiConfigState, String> {
    let gemini_home = gemini_cli_home()
        .ok_or_else(|| "HOME, USERPROFILE, or GEMINI_CLI_HOME is not set".to_string())?;
    let system_defaults = system_defaults_path();
    let user_settings = gemini_home.join(".gemini/settings.json");
    let system_settings = system_settings_path();

    let mut trust_settings = GeminiSettings::default();
    apply_optional_layer(&mut trust_settings, system_defaults.as_deref())?;
    apply_optional_layer(&mut trust_settings, Some(&user_settings))?;
    apply_optional_layer(&mut trust_settings, system_settings.as_deref())?;

    let workspace_trust = workspace
        .map(|workspace| {
            workspace_trust(workspace, &gemini_home, trust_settings.folder_trust_enabled)
        })
        .transpose()?
        .unwrap_or_default();

    let mut settings = GeminiSettings::default();
    apply_optional_layer(&mut settings, system_defaults.as_deref())?;
    apply_optional_layer(&mut settings, Some(&user_settings))?;
    if workspace_trust.trusted == Some(true)
        && let Some(workspace) = workspace
    {
        apply_optional_layer(
            &mut settings,
            Some(&workspace.join(".gemini/settings.json")),
        )?;
    }
    apply_optional_layer(&mut settings, system_settings.as_deref())?;

    Ok(GeminiConfigState {
        settings,
        workspace_trusted: workspace_trust.trusted,
        trust_source: workspace_trust.source,
        trusted_folders_path: workspace_trust.path,
    })
}

fn apply_optional_layer(settings: &mut GeminiSettings, path: Option<&Path>) -> Result<(), String> {
    let Some(path) = path else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
    let value = parse_json_with_comments(&raw)
        .map_err(|error| format!("invalid JSON in '{}': {error}", path.display()))?;
    apply_settings_value(settings, path, &value)?;
    settings.loaded_paths.push(path.display().to_string());
    Ok(())
}

fn apply_settings_value(
    settings: &mut GeminiSettings,
    path: &Path,
    value: &Value,
) -> Result<(), String> {
    let root = value
        .as_object()
        .ok_or_else(|| format!("'{}' must contain a JSON object", path.display()))?;
    if let Some(skills) = root.get("skills") {
        let skills = skills
            .as_object()
            .ok_or_else(|| format!("skills in '{}' must be an object", path.display()))?;
        if let Some(enabled) = skills.get("enabled") {
            settings.skills_enabled = enabled.as_bool().ok_or_else(|| {
                format!("skills.enabled in '{}' must be a boolean", path.display())
            })?;
        }
        if let Some(disabled) = skills.get("disabled") {
            settings.disabled_skills = string_array(disabled, path, "skills.disabled")?;
        }
    }
    if let Some(admin) = root.get("admin") {
        let admin = admin
            .as_object()
            .ok_or_else(|| format!("admin in '{}' must be an object", path.display()))?;
        if let Some(skills) = admin.get("skills") {
            let skills = skills
                .as_object()
                .ok_or_else(|| format!("admin.skills in '{}' must be an object", path.display()))?;
            if let Some(enabled) = skills.get("enabled") {
                settings.admin_skills_enabled = enabled.as_bool().ok_or_else(|| {
                    format!(
                        "admin.skills.enabled in '{}' must be a boolean",
                        path.display()
                    )
                })?;
            }
        }
    }
    if let Some(security) = root.get("security") {
        let security = security
            .as_object()
            .ok_or_else(|| format!("security in '{}' must be an object", path.display()))?;
        if let Some(folder_trust) = security.get("folderTrust") {
            let folder_trust = folder_trust.as_object().ok_or_else(|| {
                format!(
                    "security.folderTrust in '{}' must be an object",
                    path.display()
                )
            })?;
            if let Some(enabled) = folder_trust.get("enabled") {
                settings.folder_trust_enabled = enabled.as_bool().ok_or_else(|| {
                    format!(
                        "security.folderTrust.enabled in '{}' must be a boolean",
                        path.display()
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn string_array(value: &Value, path: &Path, field: &str) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{field} in '{}' must be an array", path.display()))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{field} in '{}' must contain strings", path.display()))
        })
        .collect()
}

fn workspace_trust(
    workspace: &Path,
    gemini_home: &Path,
    folder_trust_enabled: bool,
) -> Result<WorkspaceTrust, String> {
    if env::var("GEMINI_CLI_TRUST_WORKSPACE").as_deref() == Ok("true") {
        return Ok(WorkspaceTrust {
            trusted: Some(true),
            source: Some("env"),
            path: None,
        });
    }
    if !folder_trust_enabled {
        return Ok(WorkspaceTrust {
            trusted: Some(true),
            source: Some("disabled"),
            path: None,
        });
    }
    let path = env::var_os("GEMINI_CLI_TRUSTED_FOLDERS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| gemini_home.join(".gemini/trustedFolders.json"));
    if !path.exists() {
        return Ok(WorkspaceTrust {
            trusted: None,
            source: None,
            path: Some(path),
        });
    }
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
    let value = parse_json_with_comments(&raw)
        .map_err(|error| format!("invalid JSON in '{}': {error}", path.display()))?;
    let rules = value
        .as_object()
        .ok_or_else(|| format!("'{}' must contain a JSON object", path.display()))?;
    let workspace = normalize_existing_or_raw(workspace);
    let mut longest: Option<(usize, bool)> = None;
    for (raw_path, level) in rules {
        let level = level.as_str().ok_or_else(|| {
            format!(
                "trust level for '{raw_path}' in '{}' must be a string",
                path.display()
            )
        })?;
        let rule_path = PathBuf::from(raw_path);
        let effective = match level {
            "TRUST_FOLDER" => rule_path.clone(),
            "TRUST_PARENT" => rule_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(rule_path.clone()),
            "DO_NOT_TRUST" => rule_path.clone(),
            _ => {
                return Err(format!(
                    "invalid trust level '{level}' for '{raw_path}' in '{}'",
                    path.display()
                ));
            }
        };
        let effective = normalize_existing_or_raw(&effective);
        if workspace.starts_with(&effective) {
            let candidate = (raw_path.len(), level != "DO_NOT_TRUST");
            if longest.is_none_or(|current| candidate.0 > current.0) {
                longest = Some(candidate);
            }
        }
    }
    let trusted = longest.map(|(_, trusted)| trusted);
    let source = trusted.map(|_| "file");
    Ok(WorkspaceTrust {
        trusted,
        source,
        path: Some(path),
    })
}

fn gemini_cli_home() -> Option<PathBuf> {
    env::var_os("GEMINI_CLI_HOME")
        .map(PathBuf::from)
        .or_else(home_dir)
}

fn parse_json_with_comments(raw: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(&strip_json_comments(raw))
}

fn strip_json_comments(raw: &str) -> String {
    let mut stripped = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            stripped.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            stripped.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for comment in chars.by_ref() {
                if comment == '\n' {
                    stripped.push('\n');
                    break;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut previous = '\0';
            for comment in chars.by_ref() {
                if comment == '\n' {
                    stripped.push('\n');
                }
                if previous == '*' && comment == '/' {
                    break;
                }
                previous = comment;
            }
        } else {
            stripped.push(ch);
        }
    }

    stripped
}

fn system_defaults_path() -> Option<PathBuf> {
    system_config_path("GEMINI_CLI_SYSTEM_DEFAULTS_PATH", "system-defaults.json")
}

fn system_settings_path() -> Option<PathBuf> {
    system_config_path("GEMINI_CLI_SYSTEM_SETTINGS_PATH", "settings.json")
}

fn system_config_path(env_var: &str, file_name: &str) -> Option<PathBuf> {
    env::var_os(env_var)
        .map(PathBuf::from)
        .or_else(|| default_system_config_dir().map(|path| path.join(file_name)))
}

#[cfg(target_os = "linux")]
fn default_system_config_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/etc/gemini-cli"))
}

#[cfg(target_os = "macos")]
fn default_system_config_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/Library/Application Support/GeminiCli"))
}

#[cfg(target_os = "windows")]
fn default_system_config_dir() -> Option<PathBuf> {
    env::var_os("ProgramData")
        .map(PathBuf::from)
        .map(|path| path.join("gemini-cli"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn default_system_config_dir() -> Option<PathBuf> {
    None
}
