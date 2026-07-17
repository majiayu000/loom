use std::fs::{self, File, Metadata};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde_json::Value;

use crate::types::ErrorCode;

use super::super::super::CommandFailure;
use super::super::super::helpers::map_io;
use super::{
    Agent, ParserState, SourceAuthority, parse_agent_record, session_hash_for_text, stream,
};

pub(super) fn authority(
    path: &Path,
    metadata: &Metadata,
) -> std::result::Result<SourceAuthority, CommandFailure> {
    let modified_nanos = metadata
        .modified()
        .map_err(map_io)?
        .duration_since(UNIX_EPOCH)
        .map_err(|err| map_io(std::io::Error::other(err)))?
        .as_nanos();
    Ok(SourceAuthority {
        len: metadata.len(),
        modified_nanos,
        path_rank: path.to_string_lossy().into_owned(),
    })
}

pub(super) fn canonical_identity(
    agent: Agent,
    home: &Path,
    canonical_source: &Path,
    file: &mut File,
) -> std::result::Result<String, CommandFailure> {
    let canonical_home = fs::canonicalize(home).map_err(map_io)?;
    let relative = canonical_source
        .strip_prefix(&canonical_home)
        .map_err(|_| {
            CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "telemetry source must resolve below the selected agent home",
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");
    if agent == Agent::Codex && relative == "history.jsonl" {
        return Ok("history".to_string());
    }
    if let Some(value) = native_session_record(agent, file, None, SessionRecordSelection::First)?
        && let Some(session_id) = native_session_id(agent, &value)
    {
        return Ok(format!("session:{}", session_hash_for_text(session_id)));
    }
    Ok(format!("path:{relative}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionRecordSelection {
    First,
    Latest,
}

fn native_session_id(agent: Agent, value: &Value) -> Option<&str> {
    let session_id = match agent {
        Agent::Claude => value.get("sessionId").and_then(Value::as_str),
        Agent::Codex => (value.get("type").and_then(Value::as_str) == Some("session_meta"))
            .then(|| value.get("payload"))
            .flatten()
            .and_then(|payload| payload.get("session_id").or_else(|| payload.get("id")))
            .and_then(Value::as_str),
    };
    session_id.filter(|session_id| !session_id.is_empty())
}

fn is_native_session_boundary(agent: Agent, value: &Value) -> bool {
    match agent {
        Agent::Claude => native_session_id(agent, value).is_some(),
        Agent::Codex => value.get("type").and_then(Value::as_str) == Some("session_meta"),
    }
}

fn native_session_record(
    agent: Agent,
    file: &mut File,
    scan_end: Option<u64>,
    selection: SessionRecordSelection,
) -> std::result::Result<Option<Value>, CommandFailure> {
    file.seek(SeekFrom::Start(0)).map_err(map_io)?;
    let mut reader = BufReader::new(file);
    let mut raw = Vec::new();
    let mut offset = 0u64;
    let mut selected = None;
    loop {
        if scan_end.is_some_and(|end| offset >= end) {
            break;
        }
        let (consumed, complete) = match stream::read_record(&mut reader, &mut raw)? {
            stream::RecordStatus::Complete { consumed } => (consumed, true),
            stream::RecordStatus::Oversized { consumed } => (consumed, false),
            stream::RecordStatus::Partial { .. } | stream::RecordStatus::Eof => break,
        };
        offset = offset.checked_add(consumed).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "telemetry session preamble offset overflow",
            )
        })?;
        if scan_end.is_some_and(|end| offset > end) {
            break;
        }
        if complete && let Ok(value) = serde_json::from_slice::<Value>(&raw) {
            if selection == SessionRecordSelection::First
                && native_session_id(agent, &value).is_some()
            {
                return Ok(Some(value));
            }
            if selection == SessionRecordSelection::Latest
                && is_native_session_boundary(agent, &value)
            {
                selected = Some(value);
            }
        }
    }
    Ok(selected)
}

pub(super) fn parser_state_before(
    agent: Agent,
    file: &mut File,
    context_offset: u64,
    target_offset: u64,
) -> std::result::Result<ParserState, CommandFailure> {
    let mut state = ParserState::default();
    if target_offset == 0 || agent == Agent::Claude {
        return Ok(state);
    }
    if context_offset > target_offset {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "telemetry ingest parser context exceeds committed offset",
        ));
    }
    if context_offset > 0
        && let Some(value) = native_session_record(
            agent,
            file,
            Some(context_offset),
            SessionRecordSelection::Latest,
        )?
    {
        let _ = parse_agent_record(agent, &value, &mut state);
    }
    file.seek(SeekFrom::Start(context_offset)).map_err(map_io)?;
    let mut reader = BufReader::new(file);
    let mut raw = Vec::new();
    let mut offset = context_offset;
    while offset < target_offset {
        let consumed = match stream::read_record(&mut reader, &mut raw)? {
            stream::RecordStatus::Complete { consumed } => consumed,
            stream::RecordStatus::Oversized { consumed } => consumed,
            stream::RecordStatus::Partial { .. } | stream::RecordStatus::Eof => {
                return Err(map_io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "telemetry source changed while rebuilding parser context",
                )));
            }
        };
        offset = offset.checked_add(consumed).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "telemetry ingest parser context offset overflow",
            )
        })?;
        if offset > target_offset {
            return Err(CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "telemetry ingest checkpoint is not at a record boundary",
            ));
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&raw) {
            let _ = parse_agent_record(agent, &value, &mut state);
        }
    }
    Ok(state)
}
