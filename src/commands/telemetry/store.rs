use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

use crate::fs_util::{append_jsonl_raw, write_atomic};
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::helpers::{map_arg, map_io, validate_skill_name};
use super::super::{CommandFailure, helpers};
use super::model::{
    TELEMETRY_SCHEMA_VERSION, TelemetryConfig, TelemetryEvent, TelemetryEventDraft,
    TelemetryPrivacy,
};

#[derive(Debug, Clone)]
pub(super) struct TelemetryLogEntry {
    pub(super) bytes: usize,
    pub(super) event: TelemetryEvent,
}

#[derive(Debug, Clone)]
pub(super) struct MalformedTelemetryLine {
    pub(super) line: usize,
    pub(super) raw: String,
    pub(super) bytes: usize,
    pub(super) error: String,
}

#[derive(Debug, Default)]
pub(super) struct TelemetryLog {
    pub(super) events: Vec<TelemetryLogEntry>,
    pub(super) malformed: Vec<MalformedTelemetryLine>,
}

pub(super) fn telemetry_dir(ctx: &AppContext) -> PathBuf {
    ctx.state_dir.join("telemetry")
}

pub(super) fn config_path(ctx: &AppContext) -> PathBuf {
    telemetry_dir(ctx).join("config.json")
}

pub(super) fn events_path(ctx: &AppContext) -> PathBuf {
    telemetry_dir(ctx).join("events.jsonl")
}

pub(super) fn read_config(
    ctx: &AppContext,
) -> std::result::Result<Option<TelemetryConfig>, CommandFailure> {
    let path = config_path(ctx);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(map_io(err)),
    };
    let config: TelemetryConfig = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("invalid telemetry config '{}': {err}", path.display()),
        )
    })?;
    if config.schema_version != TELEMETRY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported telemetry config schema_version {}; expected {}",
                config.schema_version, TELEMETRY_SCHEMA_VERSION
            ),
        ));
    }
    Ok(Some(config))
}

pub(super) fn write_config(
    ctx: &AppContext,
    config: &TelemetryConfig,
) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string_pretty(config).map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to encode telemetry config: {err}"),
        )
    })? + "\n";
    write_atomic(&config_path(ctx), &raw).map_err(map_io)
}

pub(crate) fn append_event_if_enabled(
    ctx: &AppContext,
    draft: TelemetryEventDraft,
) -> std::result::Result<Option<TelemetryEvent>, CommandFailure> {
    let Some(config) = read_config(ctx)? else {
        return Ok(None);
    };
    if !config.enabled {
        return Ok(None);
    }
    let _workspace = ctx
        .lock_workspace()
        .map_err(super::super::helpers::map_lock)?;
    let Some(config) = read_config(ctx)? else {
        return Ok(None);
    };
    if !config.enabled {
        return Ok(None);
    }
    let event = redacted_event_from_draft(draft)?;
    validate_event(&event)?;
    let raw = serde_json::to_string(&event).map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to encode telemetry event: {err}"),
        )
    })?;
    append_jsonl_raw(&events_path(ctx), &raw).map_err(map_io)?;
    Ok(Some(event))
}

pub(super) fn read_event_log(
    ctx: &AppContext,
) -> std::result::Result<TelemetryLog, CommandFailure> {
    let path = events_path(ctx);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(TelemetryLog::default());
        }
        Err(err) => return Err(map_io(err)),
    };
    let mut log = TelemetryLog::default();
    for (index, line) in raw.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let bytes = line.len() + 1;
        match serde_json::from_str::<TelemetryEvent>(trimmed) {
            Ok(event) => {
                if let Err(err) = validate_event(&event) {
                    log.malformed.push(MalformedTelemetryLine {
                        line: line_no,
                        raw: line.to_string(),
                        bytes,
                        error: err.message,
                    });
                } else {
                    log.events.push(TelemetryLogEntry { bytes, event });
                }
            }
            Err(err) => log.malformed.push(MalformedTelemetryLine {
                line: line_no,
                raw: line.to_string(),
                bytes,
                error: err.to_string(),
            }),
        }
    }
    Ok(log)
}

pub(super) fn parse_cutoff(
    label: &str,
    raw: Option<&str>,
) -> std::result::Result<Option<DateTime<Utc>>, CommandFailure> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if let Ok(value) = DateTime::parse_from_rfc3339(raw) {
        return Ok(Some(value.with_timezone(&Utc)));
    }
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|err| {
        helpers::map_arg(anyhow::anyhow!(
            "{label} must be RFC3339 or YYYY-MM-DD; failed to parse '{raw}': {err}"
        ))
    })?;
    let Some(naive) = date.and_hms_opt(0, 0, 0) else {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("{label} date is outside supported range"),
        ));
    };
    Ok(Some(Utc.from_utc_datetime(&naive)))
}

pub(super) fn workspace_hash_for_path(path: &Path) -> String {
    hash_identity("workspace", &normalize_path_label(path))
}

pub(super) fn task_hash_for_text(task: &str) -> String {
    hash_identity("task", task)
}

pub(super) fn purge_token(before: Option<DateTime<Utc>>, count: usize, bytes: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"loom.telemetry.purge.v1\n");
    hasher.update(
        before
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "all".to_string())
            .as_bytes(),
    );
    hasher.update(b"\n");
    hasher.update(count.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(bytes.to_string().as_bytes());
    format!("purge-{}", &to_hex(&hasher.finalize())[..16])
}

pub(super) fn output_path_outside_state(
    ctx: &AppContext,
    output: &Path,
) -> std::result::Result<PathBuf, CommandFailure> {
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir().map_err(map_io)?.join(output)
    };
    let output = normalize_lexical(&output);
    let state = normalize_lexical(&ctx.state_dir);
    if output == state
        || output.starts_with(&state)
        || output_parent_resolves_into_state(ctx, &output)?
    {
        return Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            format!(
                "telemetry export output '{}' must not overwrite registry state under '{}'",
                output.display(),
                state.display()
            ),
        ));
    }
    Ok(output)
}

fn output_parent_resolves_into_state(
    ctx: &AppContext,
    output: &Path,
) -> std::result::Result<bool, CommandFailure> {
    let Some(parent) = output.parent() else {
        return Ok(false);
    };
    let Ok(state) = fs::canonicalize(&ctx.state_dir) else {
        return Ok(false);
    };
    let parent = canonicalize_existing_prefix(parent)?;
    Ok(parent == state || parent.starts_with(&state))
}

fn canonicalize_existing_prefix(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    let mut cursor = path;
    let mut suffix = Vec::<OsString>::new();
    loop {
        if fs::symlink_metadata(cursor).is_ok() {
            let mut resolved = fs::canonicalize(cursor).map_err(map_io)?;
            for component in suffix.iter().rev() {
                resolved.push(component);
            }
            return Ok(normalize_lexical(&resolved));
        }
        let Some(name) = cursor.file_name() else {
            return Ok(normalize_lexical(path));
        };
        suffix.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return Ok(normalize_lexical(path));
        };
        cursor = parent;
    }
}

fn redacted_event_from_draft(
    draft: TelemetryEventDraft,
) -> std::result::Result<TelemetryEvent, CommandFailure> {
    if let Some(skill) = draft.skill_id.as_deref() {
        validate_skill_name(skill).map_err(map_arg)?;
    }
    if let Some(skillset) = draft.skillset_id.as_deref() {
        validate_skill_name(skillset).map_err(map_arg)?;
    }
    if let Some(agent) = draft.agent.as_deref()
        && !agent
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("agent id '{agent}' must match [a-z0-9_-]+"),
        ));
    }
    Ok(TelemetryEvent {
        schema_version: TELEMETRY_SCHEMA_VERSION,
        event_id: format!("evt_{}", uuid::Uuid::new_v4()),
        event_type: draft.event_type,
        skill_id: draft.skill_id,
        skillset_id: draft.skillset_id,
        agent: draft.agent,
        workspace_hash: draft
            .workspace
            .as_ref()
            .map(|path| workspace_hash_for_path(path)),
        session_id_hash: draft
            .session_id
            .as_ref()
            .map(|session| hash_identity("session", session)),
        task_hash: draft.task.as_ref().map(|task| task_hash_for_text(task)),
        timestamp: draft.timestamp,
        metrics: draft.metrics,
        privacy: TelemetryPrivacy::default(),
    })
}

fn validate_event(event: &TelemetryEvent) -> std::result::Result<(), CommandFailure> {
    if event.schema_version != TELEMETRY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported telemetry event schema_version {}; expected {}",
                event.schema_version, TELEMETRY_SCHEMA_VERSION
            ),
        ));
    }
    if !event.event_id.starts_with("evt_") {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry event_id must start with 'evt_'",
        ));
    }
    if event.privacy.raw_prompt_stored || event.privacy.raw_code_stored || !event.privacy.redacted {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry events must be redacted before persistence",
        ));
    }
    Ok(())
}

fn hash_identity(kind: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"loom.telemetry.identity.v1\n");
    hasher.update(kind.as_bytes());
    hasher.update(b"\n");
    hasher.update(value.as_bytes());
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

fn normalize_path_label(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    absolute
        .canonicalize()
        .unwrap_or_else(|_| normalize_lexical(&absolute))
        .display()
        .to_string()
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::Prefix(_) => out.push(component.as_os_str()),
        }
    }
    out
}
