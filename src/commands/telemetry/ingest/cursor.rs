use std::collections::BTreeMap;
use std::fs;
use std::fs::Metadata;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::fs_util::write_atomic;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::super::CommandFailure;
use super::super::super::helpers::map_io;

const CURSOR_SCHEMA_VERSION: u32 = 1;
const CHECKPOINT_SCHEMA_VERSION: u32 = 1;
const BOUNDARY_BYTES: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IngestCursor {
    #[serde(default = "cursor_schema_version")]
    pub(super) schema_version: u32,
    #[serde(default)]
    pub(super) sources: BTreeMap<String, SourceCheckpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceCheckpoint {
    pub(super) schema_version: u32,
    pub(super) generation_token: String,
    pub(super) committed_offset: u64,
    pub(super) boundary_hash: String,
    pub(super) covered_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResetReason {
    Truncated,
    GenerationChanged,
    BoundaryMismatch,
}

impl ResetReason {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::GenerationChanged => "generation_changed",
            Self::BoundaryMismatch => "boundary_mismatch",
        }
    }
}

pub(super) struct ScanWindow {
    pub(super) start: usize,
    pub(super) complete_end: usize,
    pub(super) pending_partial: bool,
    pub(super) reset_reason: Option<ResetReason>,
    pub(super) covered_since: Option<DateTime<Utc>>,
}

pub(super) fn cursor_path(ctx: &AppContext) -> PathBuf {
    super::super::store::telemetry_dir(ctx).join("ingest_cursor.json")
}

pub(super) fn read_cursor(ctx: &AppContext) -> std::result::Result<IngestCursor, CommandFailure> {
    let path = cursor_path(ctx);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(IngestCursor {
                schema_version: CURSOR_SCHEMA_VERSION,
                sources: BTreeMap::new(),
            });
        }
        Err(err) => return Err(map_io(err)),
    };
    let cursor: IngestCursor = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("invalid telemetry ingest cursor: {err}"),
        )
    })?;
    if cursor.schema_version != CURSOR_SCHEMA_VERSION
        || cursor
            .sources
            .values()
            .any(|checkpoint| checkpoint.schema_version != CHECKPOINT_SCHEMA_VERSION)
    {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "unsupported telemetry ingest cursor schema_version",
        ));
    }
    Ok(cursor)
}

pub(super) fn write_cursor_locked(
    ctx: &AppContext,
    cursor: &IngestCursor,
) -> std::result::Result<(), CommandFailure> {
    let raw = serde_json::to_string_pretty(cursor).map_err(|err| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("failed to encode telemetry ingest cursor: {err}"),
        )
    })? + "\n";
    write_atomic(&cursor_path(ctx), &raw).map_err(map_io)
}

pub(super) fn logical_source_key(agent: &str, canonical_identity: &str) -> String {
    hash_fields(
        "loom.telemetry.logical-source.v1",
        &[agent, canonical_identity],
    )
}

pub(super) fn scan_window(
    bytes: &[u8],
    current_generation_identity: &str,
    checkpoint: Option<&SourceCheckpoint>,
    requested_since: Option<DateTime<Utc>>,
) -> std::result::Result<ScanWindow, CommandFailure> {
    let mut start = 0usize;
    let mut reset_reason = None;
    let mut covered_since = requested_since;
    if let Some(checkpoint) = checkpoint {
        covered_since = min_since(checkpoint.covered_since, requested_since);
        let offset = usize::try_from(checkpoint.committed_offset).map_err(|_| {
            CommandFailure::new(
                ErrorCode::SchemaMismatch,
                "telemetry ingest committed_offset exceeds platform size",
            )
        })?;
        let earlier_backfill = match (requested_since, checkpoint.covered_since) {
            (None, Some(_)) => true,
            (Some(requested), Some(covered)) => requested < covered,
            _ => false,
        };
        if offset > bytes.len() {
            reset_reason = Some(ResetReason::Truncated);
        } else if checkpoint.generation_token
            != generation_token(current_generation_identity, bytes, offset)
        {
            reset_reason = Some(ResetReason::GenerationChanged);
        } else if checkpoint.boundary_hash != boundary_hash(bytes, offset) {
            reset_reason = Some(ResetReason::BoundaryMismatch);
        } else if !earlier_backfill {
            start = offset;
        }
    }
    let tail = &bytes[start..];
    let complete_end = tail
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(start, |index| start + index + 1);
    Ok(ScanWindow {
        start,
        complete_end,
        pending_partial: complete_end < bytes.len(),
        reset_reason,
        covered_since,
    })
}

pub(super) fn checkpoint_for(
    bytes: &[u8],
    generation_identity: &str,
    committed_offset: usize,
    covered_since: Option<DateTime<Utc>>,
) -> std::result::Result<SourceCheckpoint, CommandFailure> {
    let committed_offset = u64::try_from(committed_offset).map_err(|_| {
        CommandFailure::new(
            ErrorCode::InternalError,
            "telemetry ingest committed_offset overflow",
        )
    })?;
    Ok(SourceCheckpoint {
        schema_version: CHECKPOINT_SCHEMA_VERSION,
        generation_token: generation_token(generation_identity, bytes, committed_offset as usize),
        committed_offset,
        boundary_hash: boundary_hash(bytes, committed_offset as usize),
        covered_since,
    })
}

#[cfg(unix)]
pub(super) fn source_generation_identity(metadata: &Metadata) -> String {
    use std::os::unix::fs::MetadataExt;

    hash_fields(
        "loom.telemetry.source-generation.v2",
        &[&metadata.dev().to_string(), &metadata.ino().to_string()],
    )
}

#[cfg(windows)]
pub(super) fn source_generation_identity(metadata: &Metadata) -> String {
    use std::os::windows::fs::MetadataExt;

    hash_fields(
        "loom.telemetry.source-generation.v2",
        &[
            &metadata
                .volume_serial_number()
                .unwrap_or_default()
                .to_string(),
            &metadata.file_index().unwrap_or_default().to_string(),
            &metadata.creation_time().to_string(),
        ],
    )
}

#[cfg(not(any(unix, windows)))]
pub(super) fn source_generation_identity(metadata: &Metadata) -> String {
    let created = metadata
        .created()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(
            || "unknown".to_string(),
            |value| value.as_nanos().to_string(),
        );
    hash_fields("loom.telemetry.source-generation.v2", &[&created])
}

fn generation_token(identity: &str, bytes: &[u8], committed_offset: usize) -> String {
    hash_fields(
        "loom.telemetry.source-generation.v2",
        &[
            identity,
            &hash_bytes(
                "loom.telemetry.source-generation-content.v1",
                &bytes[..committed_offset],
            ),
        ],
    )
}

fn boundary_hash(bytes: &[u8], offset: usize) -> String {
    let start = offset.saturating_sub(BOUNDARY_BYTES);
    hash_bytes("loom.telemetry.source-boundary.v1", &bytes[start..offset])
}

fn min_since(
    current: Option<DateTime<Utc>>,
    requested: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (current, requested) {
        (Some(current), Some(requested)) => Some(current.min(requested)),
        (Some(_), None) => None,
        (None, Some(requested)) => Some(requested),
        (None, None) => None,
    }
}

fn cursor_schema_version() -> u32 {
    CURSOR_SCHEMA_VERSION
}

pub(super) fn hash_fields(domain: &str, fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    for field in fields {
        hasher.update(b"\n");
        hasher.update(field.len().to_string().as_bytes());
        hasher.update(b":");
        hasher.update(field.as_bytes());
    }
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

fn hash_bytes(domain: &str, bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update(b"\n");
    hasher.update(bytes.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(bytes);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::{ResetReason, checkpoint_for, scan_window};

    #[test]
    fn partial_tail_does_not_advance_checkpoint() {
        let bytes = b"one\ntwo";
        let window = scan_window(bytes, "generation-a", None, None).expect("scan window");
        assert_eq!(window.complete_end, 4);
        assert!(window.pending_partial);
    }

    #[test]
    fn earlier_since_rewinds_a_valid_checkpoint() {
        let bytes = b"one\ntwo\n";
        let recent = Utc.with_ymd_and_hms(2026, 7, 2, 0, 0, 0).unwrap();
        let older = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let checkpoint =
            checkpoint_for(bytes, "generation-a", bytes.len(), Some(recent)).expect("checkpoint");
        let window = scan_window(bytes, "generation-a", Some(&checkpoint), Some(older))
            .expect("scan window");
        assert_eq!(window.start, 0);
        assert_eq!(window.covered_since, Some(older));
    }

    #[test]
    fn same_size_middle_rewrite_resets_checkpoint() {
        let mut bytes = b"first\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'a', 10_000));
        bytes.extend_from_slice(b"\ntail\n");
        let checkpoint =
            checkpoint_for(&bytes, "generation-a", bytes.len(), None).expect("checkpoint");
        let middle = bytes.len() / 2;
        bytes[middle] = b'b';
        let window =
            scan_window(&bytes, "generation-a", Some(&checkpoint), None).expect("scan window");
        assert_eq!(window.reset_reason, Some(ResetReason::GenerationChanged));
        assert_eq!(window.start, 0);
    }

    #[test]
    fn replacement_generation_resets_identical_bytes() {
        let bytes = b"one\ntwo\n";
        let checkpoint =
            checkpoint_for(bytes, "generation-a", bytes.len(), None).expect("checkpoint");
        let window =
            scan_window(bytes, "generation-b", Some(&checkpoint), None).expect("scan window");
        assert_eq!(window.reset_reason, Some(ResetReason::GenerationChanged));
        assert_eq!(window.start, 0);
    }

    #[test]
    fn truncation_resets_checkpoint() {
        let bytes = b"one\ntwo\n";
        let checkpoint =
            checkpoint_for(bytes, "generation-a", bytes.len(), None).expect("checkpoint");
        let window =
            scan_window(b"one\n", "generation-a", Some(&checkpoint), None).expect("scan window");
        assert_eq!(window.reset_reason, Some(ResetReason::Truncated));
        assert_eq!(window.start, 0);
    }
}
