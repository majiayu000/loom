use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use toml_edit::{DocumentMut, Item, value};

use crate::state::home_dir;
use crate::types::ErrorCode;

use super::helpers::map_io;
use super::{CommandFailure, codex_visibility::normalize_existing_or_raw};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexConfigView {
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
    pub(crate) entries: Vec<CodexSkillConfigEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexSkillConfigEntry {
    pub(crate) index: usize,
    pub(crate) path: Option<PathBuf>,
    pub(crate) raw_path: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexConfigMalformed {
    pub(crate) path: PathBuf,
    pub(crate) error: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexConfigPatchResult {
    pub(crate) path: PathBuf,
    pub(crate) patched_entries: Vec<usize>,
    pub(crate) restart_required: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum CodexConfigLoad {
    Parsed(CodexConfigView),
    Malformed(CodexConfigMalformed),
}

impl CodexSkillConfigEntry {
    pub(crate) fn is_disabled(&self) -> bool {
        self.enabled == Some(false)
    }

    pub(crate) fn matches_skill(&self, skill: &str, skill_file: &Path) -> Option<&'static str> {
        if self.path.as_ref().is_some_and(|path| {
            normalize_existing_or_raw(path) == normalize_existing_or_raw(skill_file)
        }) {
            return Some("path");
        }
        if self.name.as_deref() == Some(skill) {
            return Some("name");
        }
        None
    }
}

pub(crate) fn load_codex_config() -> std::result::Result<CodexConfigLoad, CommandFailure> {
    let path = codex_config_path()?;
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CodexConfigLoad::Parsed(CodexConfigView {
                path,
                exists: false,
                entries: Vec::new(),
            }));
        }
        Err(err) => return Err(map_io(err)),
    };

    let doc = match raw.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(err) => {
            return Ok(CodexConfigLoad::Malformed(CodexConfigMalformed {
                path,
                error: err.to_string(),
            }));
        }
    };
    Ok(CodexConfigLoad::Parsed(CodexConfigView {
        entries: extract_entries(&doc, &path),
        path,
        exists: true,
    }))
}

pub(crate) fn malformed_config_failure(error: &CodexConfigMalformed) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::SchemaMismatch,
        format!("Codex config '{}' is malformed", error.path.display()),
    );
    failure.details = serde_json::json!({
        "config_path": error.path,
        "error": error.error
    });
    failure
}

pub(crate) fn patch_disabled_entries(
    indices: &BTreeSet<usize>,
) -> std::result::Result<CodexConfigPatchResult, CommandFailure> {
    let path = codex_config_path()?;
    if indices.is_empty() {
        return Ok(CodexConfigPatchResult {
            path,
            patched_entries: Vec::new(),
            restart_required: false,
        });
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let mut doc = raw.parse::<DocumentMut>().map_err(|err| {
        let mut failure = CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("Codex config '{}' is malformed", path.display()),
        );
        failure.details = serde_json::json!({
            "config_path": path,
            "error": err.to_string()
        });
        failure
    })?;

    let configs = doc["skills"]["config"]
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "Codex config does not contain [[skills.config]] entries to patch",
            )
        })?;
    let mut patched = Vec::new();
    for index in indices {
        let Some(table) = configs.get_mut(*index) else {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                format!("Codex config entry {} disappeared before patch", index),
            ));
        };
        table["enabled"] = value(true);
        patched.push(*index);
    }

    let rendered = doc.to_string();
    rendered.parse::<DocumentMut>().map_err(|err| {
        let mut failure = CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "patched Codex config failed TOML validation",
        );
        failure.details = serde_json::json!({"error": err.to_string()});
        failure
    })?;

    let parent = path.parent().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::IoError,
            format!("Codex config '{}' has no parent directory", path.display()),
        )
    })?;
    fs::create_dir_all(parent).map_err(map_io)?;
    let tmp = parent.join(format!(
        ".{}.loom-tmp-{}",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("config.toml"),
        uuid::Uuid::new_v4().simple()
    ));
    if let Err(err) = fs::write(&tmp, rendered) {
        return Err(map_io(err));
    }
    if let Err(err) = fs::rename(&tmp, &path) {
        let _ = fs::remove_file(&tmp);
        return Err(map_io(err));
    }

    Ok(CodexConfigPatchResult {
        path,
        patched_entries: patched,
        restart_required: true,
    })
}

pub(crate) fn codex_config_path() -> std::result::Result<PathBuf, CommandFailure> {
    let path = if let Some(home) = std::env::var_os("CODEX_HOME") {
        PathBuf::from(home).join("config.toml")
    } else {
        home_dir()
            .map(|home| home.join(".codex/config.toml"))
            .ok_or_else(|| CommandFailure::new(ErrorCode::ArgInvalid, "HOME is not set"))?
    };
    if path.is_absolute() {
        return Ok(path);
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(map_io)
}

fn extract_entries(doc: &DocumentMut, config_path: &Path) -> Vec<CodexSkillConfigEntry> {
    let Some(array) = doc
        .get("skills")
        .and_then(Item::as_table)
        .and_then(|skills| skills.get("config"))
        .and_then(Item::as_array_of_tables)
    else {
        return Vec::new();
    };
    array
        .iter()
        .enumerate()
        .map(|(index, table)| {
            let raw_path = table.get("path").and_then(Item::as_str).map(str::to_string);
            let path = raw_path
                .as_deref()
                .map(|raw| config_entry_path(config_path, raw));
            CodexSkillConfigEntry {
                index,
                path,
                raw_path,
                name: table.get("name").and_then(Item::as_str).map(str::to_string),
                enabled: table.get("enabled").and_then(Item::as_bool),
            }
        })
        .collect()
}

fn config_entry_path(config_path: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}
