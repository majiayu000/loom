use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{map_arg, map_io, validate_non_empty, validate_skill_name};
use super::model::VerifyFinding;

pub(super) fn ensure_skill_source_exists(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    if !ctx.skill_path(skill).join("SKILL.md").is_file() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    Ok(())
}

pub(super) fn validate_agent_selector(
    name: &str,
    value: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_non_empty(name, value)?;
    if value.len() > 64
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("--{name} must match [A-Za-z0-9._-]{{1,64}}"),
        ));
    }
    Ok(())
}

pub(super) fn validate_artifact_id(artifact_id: &str) -> std::result::Result<(), CommandFailure> {
    if artifact_id.is_empty() || artifact_id == "." || artifact_id == ".." {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--artifact must be a non-empty safe path segment",
        ));
    }
    if artifact_id.len() > 160
        || !artifact_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--artifact must match [A-Za-z0-9._-]{1,160}",
        ));
    }
    Ok(())
}

pub(super) fn compiled_skill_root(ctx: &AppContext, skill: &str) -> PathBuf {
    ctx.state_dir.join("compiled/skills").join(skill)
}

pub(super) fn artifact_ids(root: &Path) -> std::result::Result<Vec<String>, CommandFailure> {
    let mut ids = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(err) => return Err(map_io(err)),
    };
    for entry in entries {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().map_err(map_io)?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        validate_artifact_id(&id)?;
        ids.push(id);
    }
    ids.sort();
    Ok(ids)
}

pub(super) fn stable_json<T: Serialize>(value: &T) -> std::result::Result<String, CommandFailure> {
    let mut raw = serde_json::to_string_pretty(value).map_err(map_io)?;
    raw.push('\n');
    Ok(raw)
}

pub(super) fn estimate_tokens(raw: &str) -> usize {
    raw.chars()
        .count()
        .div_ceil(4)
        .max(raw.split_whitespace().count())
}

pub(super) fn digest_bytes_prefixed(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

pub(super) fn update_digest_field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    hasher.update(b"\0");
}

pub(super) fn sanitize_artifact_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub(super) fn slash_path(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn frontmatter_value<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = raw.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        let Some((candidate, value)) = trimmed.split_once(':') else {
            continue;
        };
        if candidate.trim().eq_ignore_ascii_case(key) {
            return Some(value.trim().trim_matches('"').trim_matches('\''));
        }
    }
    None
}

pub(super) fn push_unique_limited(values: &mut Vec<String>, value: &str, limit: usize) {
    if values.len() >= limit || values.iter().any(|existing| existing == value) {
        return;
    }
    values.push(value.to_string());
}

pub(super) fn push_compile_finding(
    findings: &mut Vec<VerifyFinding>,
    id: &str,
    severity: &str,
    message: impl Into<String>,
    details: Value,
) {
    findings.push(VerifyFinding {
        id: id.to_string(),
        severity: severity.to_string(),
        message: message.into(),
        details,
    });
}
