use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::state::{home_dir, load_dotenv_file};

pub(crate) const INVALID_CONFIG: &str = "Gemini CLI settings or trust configuration is invalid";

#[derive(Debug)]
pub(crate) struct GeminiSettings {
    pub(crate) skills_enabled: bool,
    pub(crate) skill_disabled: bool,
    folder_trust_enabled: bool,
    ignore_local_env: bool,
    excluded_env_vars: Vec<String>,
}

impl Default for GeminiSettings {
    fn default() -> Self {
        Self {
            skills_enabled: true,
            skill_disabled: false,
            folder_trust_enabled: true,
            ignore_local_env: false,
            excluded_env_vars: vec!["DEBUG".to_string(), "DEBUG_MODE".to_string()],
        }
    }
}

pub(crate) struct GeminiConfigState {
    pub(crate) settings: GeminiSettings,
    pub(crate) workspace_trusted: Option<bool>,
    pub(crate) runtime_home: PathBuf,
}

pub(crate) fn bootstrap_home() -> Option<PathBuf> {
    env::var("GEMINI_CLI_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(home_dir)
}

pub(crate) fn runtime_home(workspace: &Path) -> Result<PathBuf, &'static str> {
    load_config("", Some(workspace)).map(|state| state.runtime_home)
}

pub(crate) fn load_config(
    skill: &str,
    workspace: Option<&Path>,
) -> Result<GeminiConfigState, &'static str> {
    let bootstrap_home = bootstrap_home().ok_or(INVALID_CONFIG)?;
    let defaults = system_path("GEMINI_CLI_SYSTEM_DEFAULTS_PATH", "system-defaults.json");
    let user = bootstrap_home.join(".gemini/settings.json");
    let system = system_path("GEMINI_CLI_SYSTEM_SETTINGS_PATH", "settings.json");

    let mut settings = GeminiSettings::default();
    for path in [defaults.as_deref(), Some(user.as_path()), system.as_deref()] {
        apply_layer(&mut settings, path, skill)?;
    }
    let workspace_trusted = match workspace {
        Some(path) => workspace_trust(path, &bootstrap_home, settings.folder_trust_enabled)?,
        None => None,
    };
    if workspace_trusted == Some(true)
        && let Some(workspace) = workspace
    {
        apply_layer(
            &mut settings,
            Some(&workspace.join(".gemini/settings.json")),
            skill,
        )?;
        apply_layer(&mut settings, system.as_deref(), skill)?;
    }
    let runtime_home = runtime_home_after_trust(
        workspace,
        &bootstrap_home,
        workspace_trusted == Some(true),
        &settings,
    );
    Ok(GeminiConfigState {
        settings,
        workspace_trusted,
        runtime_home,
    })
}

fn runtime_home_after_trust(
    workspace: Option<&Path>,
    bootstrap_home: &Path,
    trusted: bool,
    settings: &GeminiSettings,
) -> PathBuf {
    if let Ok(value) = env::var("GEMINI_CLI_HOME")
        && !value.is_empty()
    {
        return PathBuf::from(value);
    }
    if !trusted {
        return bootstrap_home.to_path_buf();
    }
    workspace
        .and_then(|workspace| stable_env_file(workspace, bootstrap_home, settings.ignore_local_env))
        .filter(|path| {
            is_gemini_env(path)
                || !settings
                    .excluded_env_vars
                    .iter()
                    .any(|key| key == "GEMINI_CLI_HOME")
        })
        .and_then(|path| load_dotenv_file(&path).get("GEMINI_CLI_HOME").cloned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| bootstrap_home.to_path_buf())
}

fn stable_env_file(workspace: &Path, home: &Path, ignore_local_env: bool) -> Option<PathBuf> {
    let configured = find_env_file(workspace, home, ignore_local_env);
    let cli_ignored = find_env_file(workspace, home, true);
    (configured == cli_ignored).then_some(configured).flatten()
}

fn find_env_file(workspace: &Path, home: &Path, ignore_local_env: bool) -> Option<PathBuf> {
    for directory in workspace.ancestors() {
        let gemini = directory.join(".gemini/.env");
        if gemini.is_file() {
            return Some(gemini);
        }
        let generic = directory.join(".env");
        if generic.is_file() && (!ignore_local_env || directory == home) {
            return Some(generic);
        }
    }
    for candidate in [home.join(".gemini/.env"), home.join(".env")] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn is_gemini_env(path: &Path) -> bool {
    path.parent().and_then(Path::file_name) == Some(std::ffi::OsStr::new(".gemini"))
}

fn apply_layer(
    settings: &mut GeminiSettings,
    path: Option<&Path>,
    skill: &str,
) -> Result<(), &'static str> {
    let Some(path) = path else {
        return Ok(());
    };
    let Some(value) = read_json(path)? else {
        return Ok(());
    };
    if let Some(value) = lookup(&value, &["skills", "enabled"])? {
        settings.skills_enabled = value.as_bool().ok_or(INVALID_CONFIG)?;
    }
    if let Some(value) = lookup(&value, &["skills", "disabled"])? {
        for value in value.as_array().ok_or(INVALID_CONFIG)? {
            settings.skill_disabled |= value
                .as_str()
                .ok_or(INVALID_CONFIG)?
                .eq_ignore_ascii_case(skill);
        }
    }
    if let Some(value) = lookup(&value, &["security", "folderTrust", "enabled"])? {
        settings.folder_trust_enabled = value.as_bool().ok_or(INVALID_CONFIG)?;
    }
    if let Some(value) = lookup(&value, &["advanced", "ignoreLocalEnv"])? {
        settings.ignore_local_env = value.as_bool().ok_or(INVALID_CONFIG)?;
    }
    if let Some(value) = lookup(&value, &["advanced", "excludedEnvVars"])? {
        for value in value.as_array().ok_or(INVALID_CONFIG)? {
            let key = value.as_str().ok_or(INVALID_CONFIG)?;
            if !settings
                .excluded_env_vars
                .iter()
                .any(|existing| existing == key)
            {
                settings.excluded_env_vars.push(key.to_string());
            }
        }
    }
    Ok(())
}

fn lookup<'a>(value: &'a Value, path: &[&str]) -> Result<Option<&'a Value>, &'static str> {
    let mut current = value;
    for key in path {
        let object = current.as_object().ok_or(INVALID_CONFIG)?;
        let Some(next) = object.get(*key) else {
            return Ok(None);
        };
        current = next;
    }
    Ok(Some(current))
}

fn read_json(path: &Path) -> Result<Option<Value>, &'static str> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(|_| INVALID_CONFIG)?;
    serde_json::from_str(&strip_comments(&raw))
        .map(Some)
        .map_err(|_| INVALID_CONFIG)
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
    let Some(value) = read_json(&path)? else {
        return Ok(None);
    };
    let rules = value.as_object().ok_or(INVALID_CONFIG)?;
    let workspace = normalize_existing_or_raw(workspace);
    let mut longest = None;
    for (raw_path, value) in rules {
        let level = value.as_str().ok_or(INVALID_CONFIG)?;
        let rule = Path::new(raw_path);
        let effective = match level {
            "TRUST_FOLDER" | "DO_NOT_TRUST" => rule,
            "TRUST_PARENT" => rule.parent().unwrap_or(rule),
            _ => return Err(INVALID_CONFIG),
        };
        if workspace.starts_with(normalize_existing_or_raw(effective)) {
            let candidate = (raw_path.len(), level != "DO_NOT_TRUST");
            if longest.is_none_or(|current: (usize, bool)| candidate.0 > current.0) {
                longest = Some(candidate);
            }
        }
    }
    Ok(longest.map(|(_, trusted)| trusted))
}

fn normalize_existing_or_raw(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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
