use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};
use yaml_rust2::{Yaml, YamlLoader};

use super::super::codex_config::codex_config_path;

pub(crate) fn contains_word_token(text: &str, token: &str) -> bool {
    text.match_indices(token).any(|(idx, _)| {
        token_boundary(text[..idx].chars().next_back())
            && token_boundary(text[idx + token.len()..].chars().next())
    })
}

fn token_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')))
}

pub(crate) fn find_executable_on_path(tool: &str) -> Option<PathBuf> {
    if has_path_separator(tool) {
        let path = PathBuf::from(tool);
        return executable_file(&path).then_some(path);
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .flat_map(|dir| executable_candidates(&dir, tool))
            .find(|path| executable_file(path))
    })
}

fn has_path_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

#[cfg(windows)]
fn executable_candidates(dir: &Path, tool: &str) -> Vec<PathBuf> {
    if Path::new(tool).extension().is_some() {
        return vec![dir.join(tool)];
    }
    let pathext = env::var_os("PATHEXT")
        .map(|value| {
            env::split_paths(&value)
                .filter_map(|path| path.to_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            vec![
                ".COM".to_string(),
                ".EXE".to_string(),
                ".BAT".to_string(),
                ".CMD".to_string(),
            ]
        });
    std::iter::once(dir.join(tool))
        .chain(
            pathext
                .into_iter()
                .map(|ext| dir.join(format!("{tool}{ext}"))),
        )
        .collect()
}

#[cfg(not(windows))]
fn executable_candidates(dir: &Path, tool: &str) -> Vec<PathBuf> {
    vec![dir.join(tool)]
}

#[cfg(unix)]
fn executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn executable_file(path: &Path) -> bool {
    path.is_file()
}

pub(crate) fn yaml_dependency_values(raw: &str) -> Vec<(String, String)> {
    let Ok(docs) = YamlLoader::load_from_str(raw) else {
        return Vec::new();
    };
    let Some(Yaml::Hash(mapping)) = docs.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in mapping {
        let Yaml::String(key) = key else {
            continue;
        };
        if !matches!(
            key.as_str(),
            "requires_tools" | "requires_mcp" | "requires_env" | "network"
        ) {
            continue;
        }
        for value in yaml_scalar_values(value) {
            out.push((key.to_string(), value));
        }
    }
    out
}

fn yaml_scalar_values(value: &Yaml) -> Vec<String> {
    match value {
        Yaml::String(text) => vec![text.trim().to_string()],
        Yaml::Integer(number) => vec![number.to_string()],
        Yaml::Real(number) => vec![number.to_string()],
        Yaml::Boolean(flag) => vec![flag.to_string()],
        Yaml::Array(items) => items.iter().flat_map(yaml_scalar_values).collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn codex_mcp_configured(name: &str) -> Option<bool> {
    let path = codex_config_path().ok()?;
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    let doc = raw.parse::<DocumentMut>().ok()?;
    Some(
        doc.get("mcp_servers")
            .and_then(Item::as_table)
            .and_then(|servers| servers.get(name))
            .is_some_and(|item| item.as_table().is_some() || item.as_inline_table().is_some()),
    )
}
