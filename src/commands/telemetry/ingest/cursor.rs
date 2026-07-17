use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::fs::Metadata;
use std::io::{Read, Seek, SeekFrom};
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

#[cfg(test)]
pub(super) struct ScanWindow {
    pub(super) start: usize,
    pub(super) complete_end: usize,
    pub(super) pending_partial: bool,
    pub(super) reset_reason: Option<ResetReason>,
    pub(super) covered_since: Option<DateTime<Utc>>,
}

pub(super) struct FileScanWindow {
    pub(super) start: u64,
    pub(super) parser_context_offset: u64,
    pub(super) reset_reason: Option<ResetReason>,
    pub(super) covered_since: Option<DateTime<Utc>>,
    pub(super) snapshot: SourceSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceSnapshot {
    identity_hash: String,
    file_len: u64,
    modified_nanos: u128,
    change_stamp: String,
}

struct ParsedGenerationToken {
    snapshot: SourceSnapshot,
    parser_context_offset: u64,
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

#[cfg(test)]
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
    if reset_reason.is_some() {
        covered_since = requested_since;
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

#[cfg(test)]
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

pub(super) fn scan_file_window(
    file: &mut File,
    current_generation_identity: &str,
    checkpoint: Option<&SourceCheckpoint>,
    requested_since: Option<DateTime<Utc>>,
) -> std::result::Result<FileScanWindow, CommandFailure> {
    let snapshot = source_snapshot(file, current_generation_identity)?;
    let mut start = 0u64;
    let mut parser_context_offset = 0u64;
    let mut reset_reason = None;
    let mut covered_since = requested_since;
    if let Some(checkpoint) = checkpoint {
        covered_since = min_since(checkpoint.covered_since, requested_since);
        let offset = checkpoint.committed_offset;
        let earlier_backfill = match (requested_since, checkpoint.covered_since) {
            (None, Some(_)) => true,
            (Some(requested), Some(covered)) => requested < covered,
            _ => false,
        };
        if offset > snapshot.file_len {
            reset_reason = Some(ResetReason::Truncated);
        } else if let Some(previous) = parse_generation_token(&checkpoint.generation_token) {
            if previous.snapshot.identity_hash != snapshot.identity_hash {
                reset_reason = Some(ResetReason::GenerationChanged);
            } else if snapshot.file_len < previous.snapshot.file_len {
                reset_reason = Some(ResetReason::Truncated);
            } else if snapshot.file_len == previous.snapshot.file_len
                && (snapshot.modified_nanos != previous.snapshot.modified_nanos
                    || snapshot.change_stamp != previous.snapshot.change_stamp)
            {
                reset_reason = Some(ResetReason::GenerationChanged);
            } else {
                parser_context_offset = previous.parser_context_offset;
            }
        } else {
            reset_reason = Some(ResetReason::GenerationChanged);
        }
        if reset_reason.is_none() && checkpoint.boundary_hash != boundary_hash_file(file, offset)? {
            reset_reason = Some(ResetReason::BoundaryMismatch);
        } else if reset_reason.is_none() && !earlier_backfill {
            start = offset;
        }
    }
    if reset_reason.is_some() {
        covered_since = requested_since;
        parser_context_offset = 0;
    } else if start == 0 {
        parser_context_offset = 0;
    }
    Ok(FileScanWindow {
        start,
        parser_context_offset,
        reset_reason,
        covered_since,
        snapshot,
    })
}

pub(super) fn checkpoint_for_snapshot(
    file: &mut File,
    snapshot: &SourceSnapshot,
    committed_offset: u64,
    parser_context_offset: u64,
    covered_since: Option<DateTime<Utc>>,
) -> std::result::Result<SourceCheckpoint, CommandFailure> {
    Ok(SourceCheckpoint {
        schema_version: CHECKPOINT_SCHEMA_VERSION,
        generation_token: generation_token_for_snapshot(snapshot, parser_context_offset),
        committed_offset,
        boundary_hash: boundary_hash_file(file, committed_offset)?,
        covered_since,
    })
}

pub(super) fn source_snapshot(
    file: &File,
    generation_identity: &str,
) -> std::result::Result<SourceSnapshot, CommandFailure> {
    let metadata = file.metadata().map_err(map_io)?;
    let modified_nanos = metadata
        .modified()
        .map_err(map_io)?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| map_io(std::io::Error::other(err)))?
        .as_nanos();
    Ok(SourceSnapshot {
        identity_hash: hash_fields(
            "loom.telemetry.source-generation-identity.v3",
            &[generation_identity],
        ),
        file_len: metadata.len(),
        modified_nanos,
        change_stamp: source_change_stamp(file, &metadata)?,
    })
}

#[cfg(unix)]
fn source_change_stamp(
    _file: &File,
    metadata: &Metadata,
) -> std::result::Result<String, CommandFailure> {
    use std::os::unix::fs::MetadataExt;

    Ok(format!("{}:{}", metadata.ctime(), metadata.ctime_nsec()))
}

#[cfg(not(unix))]
fn source_change_stamp(
    file: &File,
    _metadata: &Metadata,
) -> std::result::Result<String, CommandFailure> {
    let mut reader = file.try_clone().map_err(map_io)?;
    let position = reader.stream_position().map_err(map_io)?;
    reader.seek(SeekFrom::Start(0)).map_err(map_io)?;
    let stamp = content_change_stamp(&mut reader)?;
    reader.seek(SeekFrom::Start(position)).map_err(map_io)?;
    Ok(stamp)
}

#[cfg(any(test, not(unix)))]
fn content_change_stamp(reader: &mut impl Read) -> std::result::Result<String, CommandFailure> {
    let mut hasher = Sha256::new();
    hasher.update(b"loom.telemetry.source-generation-content.v2\n");
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(map_io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
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

#[cfg(test)]
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

fn generation_token_for_snapshot(snapshot: &SourceSnapshot, parser_context_offset: u64) -> String {
    format!(
        "v4|{}|{}|{}|{}|{}",
        snapshot.identity_hash,
        snapshot.file_len,
        snapshot.modified_nanos,
        snapshot.change_stamp,
        parser_context_offset
    )
}

fn parse_generation_token(raw: &str) -> Option<ParsedGenerationToken> {
    let mut fields = raw.split('|');
    (fields.next()? == "v4").then_some(())?;
    let identity_hash = fields.next()?.to_string();
    let file_len = fields.next()?.parse().ok()?;
    let modified_nanos = fields.next()?.parse().ok()?;
    let change_stamp = fields.next()?.to_string();
    let parser_context_offset = fields.next()?.parse().ok()?;
    fields.next().is_none().then_some(ParsedGenerationToken {
        snapshot: SourceSnapshot {
            identity_hash,
            file_len,
            modified_nanos,
            change_stamp,
        },
        parser_context_offset,
    })
}

#[cfg(test)]
fn boundary_hash(bytes: &[u8], offset: usize) -> String {
    let start = offset.saturating_sub(BOUNDARY_BYTES);
    hash_bytes("loom.telemetry.source-boundary.v1", &bytes[start..offset])
}

fn boundary_hash_file(file: &mut File, offset: u64) -> std::result::Result<String, CommandFailure> {
    let boundary = u64::try_from(BOUNDARY_BYTES).expect("boundary size fits u64");
    let start = offset.saturating_sub(boundary);
    file.seek(SeekFrom::Start(start)).map_err(map_io)?;
    let len = usize::try_from(offset - start).map_err(|_| {
        CommandFailure::new(
            ErrorCode::InternalError,
            "telemetry ingest boundary length exceeds platform size",
        )
    })?;
    let mut buffer = vec![0u8; len];
    file.read_exact(&mut buffer).map_err(map_io)?;
    Ok(hash_bytes("loom.telemetry.source-boundary.v1", &buffer))
}

fn min_since(
    current: Option<DateTime<Utc>>,
    requested: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (current, requested) {
        (Some(current), Some(requested)) => Some(current.min(requested)),
        (Some(_), None) => None,
        (None, _) => None,
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
    use std::io::Cursor;

    use chrono::{TimeZone, Utc};

    use super::{ResetReason, checkpoint_for, content_change_stamp, scan_window};

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

    #[test]
    fn reset_coverage_comes_only_from_the_current_request() {
        let old = Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap();
        let recent = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let checkpoint =
            checkpoint_for(b"old\n", "generation-a", 4, Some(old)).expect("checkpoint");
        let window = scan_window(
            b"replacement\n",
            "generation-b",
            Some(&checkpoint),
            Some(recent),
        )
        .expect("scan window");
        assert_eq!(window.reset_reason, Some(ResetReason::GenerationChanged));
        assert_eq!(window.covered_since, Some(recent));
    }

    #[test]
    fn full_coverage_remains_absorbing_across_narrow_and_full_requests() {
        let bytes = b"one\ntwo\n";
        let narrow = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap();
        let full = checkpoint_for(bytes, "generation-a", bytes.len(), None).expect("checkpoint");
        let narrow_window =
            scan_window(bytes, "generation-a", Some(&full), Some(narrow)).expect("narrow scan");
        assert_eq!(narrow_window.start, bytes.len());
        assert_eq!(narrow_window.covered_since, None);
        let after_narrow = checkpoint_for(
            bytes,
            "generation-a",
            bytes.len(),
            narrow_window.covered_since,
        )
        .expect("narrow checkpoint");
        let full_again =
            scan_window(bytes, "generation-a", Some(&after_narrow), None).expect("full scan");
        assert_eq!(full_again.start, bytes.len());
        assert_eq!(full_again.covered_since, None);
    }

    #[test]
    fn non_unix_content_stamp_detects_same_size_rewrite_with_restored_mtime() {
        let before =
            content_change_stamp(&mut Cursor::new(b"same-size-a")).expect("before content stamp");
        let after =
            content_change_stamp(&mut Cursor::new(b"same-size-b")).expect("after content stamp");
        assert_eq!(b"same-size-a".len(), b"same-size-b".len());
        assert_ne!(before, after);
    }
}
