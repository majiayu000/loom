use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

use crate::fs_util::{append_jsonl_raw, append_lines, write_atomic};
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::helpers::{map_arg, map_io, validate_skill_name};
use super::super::{CommandFailure, helpers};
use super::model::{
    TELEMETRY_EVENT_SCHEMA_VERSION, TELEMETRY_SCHEMA_VERSION, TelemetryConfig, TelemetryEvent,
    TelemetryEventDraft, TelemetryEventType, TelemetryPrivacy, failure_category_allowed,
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

pub(super) struct AppendBatchResult {
    pub(super) appended: Vec<TelemetryEvent>,
    pub(super) duplicates: usize,
}

/// Append a validated batch while the caller holds the workspace lock.
///
/// The ingest compare-and-commit path owns locking so it can update its cursor
/// in the same critical section without entering the public locking writer.
pub(super) fn append_events_deduped_locked(
    ctx: &AppContext,
    drafts: Vec<TelemetryEventDraft>,
) -> std::result::Result<AppendBatchResult, CommandFailure> {
    ensure_event_log_append_boundary(ctx)?;
    let mut ids = read_event_log(ctx)?
        .events
        .into_iter()
        .map(|entry| entry.event.event_id)
        .collect::<BTreeSet<_>>();
    let prepared = drafts
        .into_iter()
        .map(|draft| {
            let event = redacted_event_from_draft(draft)?;
            validate_event(&event)?;
            let raw = serde_json::to_string(&event).map_err(|err| {
                CommandFailure::new(
                    ErrorCode::InternalError,
                    format!("failed to encode telemetry event: {err}"),
                )
            })?;
            Ok((event, raw))
        })
        .collect::<std::result::Result<Vec<_>, CommandFailure>>()?;
    let mut appended = Vec::new();
    let mut serialized = Vec::new();
    let mut duplicates = 0usize;
    for (event, raw) in prepared {
        if !ids.insert(event.event_id.clone()) {
            duplicates = duplicates.checked_add(1).ok_or_else(|| {
                CommandFailure::new(ErrorCode::InternalError, "duplicates counter overflow")
            })?;
            continue;
        }
        serialized.push(raw);
        appended.push(event);
    }
    append_lines_recovering(ctx, &serialized)?;
    Ok(AppendBatchResult {
        appended,
        duplicates,
    })
}

fn ensure_event_log_append_boundary(ctx: &AppContext) -> std::result::Result<(), CommandFailure> {
    let path = events_path(ctx);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(map_io(err)),
    };
    if !bytes.is_empty() && bytes.last() != Some(&b'\n') {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "telemetry event log has an unterminated tail; refusing ingest cursor advance",
        ));
    }
    Ok(())
}

fn append_lines_recovering(
    ctx: &AppContext,
    lines: &[String],
) -> std::result::Result<(), CommandFailure> {
    if lines.is_empty() {
        return Ok(());
    }
    let path = events_path(ctx);
    let original_len = match fs::metadata(&path) {
        Ok(meta) => meta.len(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(err) => return Err(map_io(err)),
    };
    if let Err(append_error) = append_lines(&path, lines) {
        let rollback = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .and_then(|file| {
                file.set_len(original_len)?;
                file.sync_all()
            });
        return match rollback {
            Ok(()) => Err(map_io(append_error)),
            Err(rollback_error) => Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!(
                    "telemetry event append failed and rollback failed: append={append_error}; rollback={rollback_error}"
                ),
            )),
        };
    }
    Ok(())
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

pub(crate) fn parse_cutoff(
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

pub(super) fn session_hash_for_text(session: &str) -> String {
    hash_identity("session", session)
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
    if let Some(observed) = draft.observed_skill_name.as_deref()
        && !observed_skill_name_allowed(observed)
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "observed skill name must be 1..=128 characters matching [A-Za-z0-9._-]",
        ));
    }
    if draft.skill_id.is_some() && draft.observed_skill_name.is_some() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "telemetry event cannot contain both skill_id and observed_skill_name",
        ));
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
        schema_version: TELEMETRY_EVENT_SCHEMA_VERSION,
        event_id: draft
            .event_id_override
            .unwrap_or_else(|| format!("evt_{}", uuid::Uuid::new_v4())),
        event_type: draft.event_type,
        skill_id: draft.skill_id,
        observed_skill_name: draft.observed_skill_name,
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
    if event.schema_version == 0 || event.schema_version > TELEMETRY_EVENT_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported telemetry event schema_version {}; expected {}",
                event.schema_version, TELEMETRY_EVENT_SCHEMA_VERSION
            ),
        ));
    }
    if !event.event_id.starts_with("evt_") {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry event_id must start with 'evt_'",
        ));
    }
    if event.observed_skill_name.is_some() && event.schema_version < 3 {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "observed_skill_name requires telemetry event schema_version 3",
        ));
    }
    if event.observed_skill_name.is_some()
        && event.event_type != TelemetryEventType::SkillInvocation
    {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "observed_skill_name is only valid for skill.invocation telemetry",
        ));
    }
    if event.skill_id.is_some() && event.observed_skill_name.is_some() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry event cannot contain both skill_id and observed_skill_name",
        ));
    }
    if event
        .observed_skill_name
        .as_deref()
        .is_some_and(|name| !observed_skill_name_allowed(name))
    {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry observed_skill_name is invalid",
        ));
    }
    if event.privacy.raw_prompt_stored || event.privacy.raw_code_stored || !event.privacy.redacted {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry events must be redacted before persistence",
        ));
    }
    let failure_category = event.metrics.failure_category.as_deref();
    if failure_category.is_some_and(|category| !failure_category_allowed(category)) {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry failure_category is unsupported",
        ));
    }
    if event.event_type == TelemetryEventType::SkillError && failure_category.is_none() {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "skill.error telemetry requires failure_category",
        ));
    }
    Ok(())
}

pub(super) fn observed_skill_name_allowed(value: &str) -> bool {
    !matches!(value, "" | "." | "..")
        && value.len() <= 128
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
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
