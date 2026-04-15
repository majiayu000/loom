use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::gitops;
use crate::types::PendingOp;

use super::{AppContext, OPS_COMPACTION_THRESHOLD};
use super::{append_lines, maybe_fault_inject, write_atomic, write_history_segment_if_missing};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum OpJournalEvent {
    Queued {
        event_id: String,
        at: DateTime<Utc>,
        op: PendingOp,
    },
    Removed {
        event_id: String,
        at: DateTime<Utc>,
        op_id: String,
        reason: String,
    },
}

impl OpJournalEvent {
    fn event_id(&self) -> &str {
        match self {
            Self::Queued { event_id, .. } | Self::Removed { event_id, .. } => event_id,
        }
    }

    fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Queued { at, .. } | Self::Removed { at, .. } => *at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OpsSnapshot {
    version: u32,
    created_at: DateTime<Utc>,
    history_events: usize,
    active_ops: Vec<PendingOp>,
}

#[derive(Debug, Default)]
struct LoadedSnapshot {
    active_ops: Vec<PendingOp>,
    history_events: usize,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct OpsReadModel {
    active_ops: BTreeMap<String, PendingOp>,
    warnings: Vec<String>,
    journal_events: usize,
    history_events: usize,
}

#[derive(Debug, Clone)]
pub struct HistoryBodySummary {
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
}

impl AppContext {
    pub fn append_pending(
        &self,
        command: &str,
        details: serde_json::Value,
        request_id: String,
    ) -> Result<PendingOp> {
        self.ensure_state_layout()?;
        let op = PendingOp::new(command, details, request_id);
        let event = OpJournalEvent::Queued {
            event_id: new_event_id(),
            at: Utc::now(),
            op: op.clone(),
        };
        self.append_journal_events(&[event])?;
        self.maybe_compact_ops_journal()?;
        Ok(op)
    }

    pub fn read_pending_report(&self) -> Result<super::PendingOpsReport> {
        let model = self.read_ops_model()?;
        let mut ops = model.active_ops.into_values().collect::<Vec<_>>();
        ops.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(super::PendingOpsReport {
            ops,
            warnings: model.warnings,
            journal_events: model.journal_events,
            history_events: model.history_events,
        })
    }

    pub fn pending_count(&self) -> Result<usize> {
        Ok(self.read_pending_report()?.ops.len())
    }

    pub fn remove_pending_ops(&self, op_ids: &BTreeSet<String>) -> Result<usize> {
        self.ensure_state_layout()?;
        if op_ids.is_empty() {
            return Ok(0);
        }

        let model = self.read_ops_model()?;
        let removable = model
            .active_ops
            .keys()
            .filter(|op_id| op_ids.contains(*op_id))
            .cloned()
            .collect::<Vec<_>>();
        if removable.is_empty() {
            return Ok(0);
        }

        let events = removable
            .iter()
            .map(|op_id| OpJournalEvent::Removed {
                event_id: new_event_id(),
                at: Utc::now(),
                op_id: op_id.clone(),
                reason: "acked".to_string(),
            })
            .collect::<Vec<_>>();
        self.append_journal_events(&events)?;
        self.maybe_compact_ops_journal()?;
        Ok(removable.len())
    }

    pub fn purge_pending(&self) -> Result<usize> {
        self.ensure_state_layout()?;
        let model = self.read_ops_model()?;
        if model.active_ops.is_empty() {
            return Ok(0);
        }

        let events = model
            .active_ops
            .keys()
            .map(|op_id| OpJournalEvent::Removed {
                event_id: new_event_id(),
                at: Utc::now(),
                op_id: op_id.clone(),
                reason: "purged".to_string(),
            })
            .collect::<Vec<_>>();
        let purged = events.len();
        self.append_journal_events(&events)?;
        self.maybe_compact_ops_journal()?;
        Ok(purged)
    }

    fn read_ops_model(&self) -> Result<OpsReadModel> {
        let snapshot = self.load_ops_snapshot()?;
        let mut active_ops = snapshot
            .active_ops
            .into_iter()
            .map(|op| (op.stable_id(), op))
            .collect::<BTreeMap<_, _>>();
        let mut warnings = snapshot.warnings;
        let mut journal_events = 0usize;

        let file = match OpenOptions::new().read(true).open(&self.pending_ops_file) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(OpsReadModel {
                    active_ops,
                    warnings,
                    journal_events,
                    history_events: snapshot.history_events,
                });
            }
            Err(err) => return Err(err).context("failed to open pending_ops.jsonl"),
        };

        for (line_no, line) in BufReader::new(file).lines().enumerate() {
            let line = line.context("failed to read pending line")?;
            if line.trim().is_empty() {
                continue;
            }
            match parse_journal_line(&line) {
                Ok(event) => {
                    apply_journal_event(&mut active_ops, event);
                    journal_events += 1;
                }
                Err(err) => warnings.push(format!(
                    "skipped malformed pending op at line {}: {}",
                    line_no + 1,
                    err
                )),
            }
        }

        Ok(OpsReadModel {
            active_ops,
            warnings,
            journal_events,
            history_events: snapshot.history_events,
        })
    }

    fn load_ops_snapshot(&self) -> Result<LoadedSnapshot> {
        let raw = match fs::read_to_string(&self.pending_ops_snapshot_file) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedSnapshot::default());
            }
            Err(err) => return Err(err).context("failed to read pending ops snapshot"),
        };

        match serde_json::from_str::<OpsSnapshot>(&raw) {
            Ok(snapshot) => Ok(LoadedSnapshot {
                active_ops: snapshot.active_ops,
                history_events: snapshot.history_events,
                warnings: Vec::new(),
            }),
            Err(err) => Ok(LoadedSnapshot {
                active_ops: Vec::new(),
                history_events: 0,
                warnings: vec![format!("ignored malformed pending ops snapshot: {}", err)],
            }),
        }
    }

    fn append_journal_events(&self, events: &[OpJournalEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let lines = events
            .iter()
            .map(|event| serde_json::to_string(event).context("failed to encode pending op event"))
            .collect::<Result<Vec<_>>>()?;
        append_lines(&self.pending_ops_file, &lines)?;
        Ok(())
    }

    fn maybe_compact_ops_journal(&self) -> Result<()> {
        let raw_journal = match fs::read_to_string(&self.pending_ops_file) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).context("failed to read pending_ops.jsonl"),
        };
        let journal_event_count = raw_journal
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if journal_event_count < OPS_COMPACTION_THRESHOLD {
            return Ok(());
        }

        let model = self.read_ops_model()?;
        let segment_path = if !raw_journal.trim().is_empty() {
            let segment_path = self
                .pending_ops_history_dir
                .join(journal_segment_name(&raw_journal)?);
            write_history_segment_if_missing(&segment_path, &raw_journal)?;
            Some(segment_path)
        } else {
            None
        };
        maybe_fault_inject("ops_compact_after_history")?;

        let snapshot = OpsSnapshot {
            version: 1,
            created_at: Utc::now(),
            history_events: model.history_events + model.journal_events,
            active_ops: model.active_ops.into_values().collect(),
        };
        let snapshot_raw = serde_json::to_string_pretty(&snapshot)
            .context("failed to encode pending ops snapshot")?;
        write_atomic(&self.pending_ops_snapshot_file, &(snapshot_raw + "\n"))
            .context("failed to write pending ops snapshot")?;
        maybe_fault_inject("ops_compact_after_snapshot")?;
        if let Some(segment_path) = segment_path.as_ref() {
            gitops::mirror_history_segment(self, segment_path, &self.pending_ops_snapshot_file)
                .context("failed to mirror pending ops history into git")?;
        }
        write_atomic(&self.pending_ops_file, "").context("failed to compact pending_ops.jsonl")?;
        Ok(())
    }
}

pub fn synthesize_snapshot_raw_from_segment_bodies(segment_bodies: &[String]) -> Result<String> {
    let mut seen_event_ids = BTreeSet::new();
    let mut ordered_events = Vec::new();

    for body in segment_bodies {
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event = parse_journal_line(trimmed)?;
            let event_id = event.event_id().to_string();
            if !seen_event_ids.insert(event_id.clone()) {
                continue;
            }
            ordered_events.push((event.at(), event_id, event));
        }
    }

    ordered_events.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut active_ops = BTreeMap::new();
    for (_, _, event) in ordered_events {
        apply_journal_event(&mut active_ops, event);
    }

    let snapshot = OpsSnapshot {
        version: 1,
        created_at: Utc::now(),
        history_events: seen_event_ids.len(),
        active_ops: active_ops.into_values().collect(),
    };

    serde_json::to_string_pretty(&snapshot).context("failed to encode synthesized ops snapshot")
}

pub fn summarize_history_body(raw: &str) -> Result<HistoryBodySummary> {
    let mut first_at = None;
    let mut last_at = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = parse_journal_line(trimmed)?;
        let at = event.at();
        first_at = Some(first_at.map_or(at, |current: DateTime<Utc>| current.min(at)));
        last_at = Some(last_at.map_or(at, |current: DateTime<Utc>| current.max(at)));
    }

    Ok(HistoryBodySummary { first_at, last_at })
}

pub fn remove_path_if_exists(path: &Path) -> Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).context("failed to stat path"),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path).context("failed to remove file/symlink")?;
    } else {
        fs::remove_dir_all(path).context("failed to remove directory")?;
    }
    Ok(())
}

fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn parse_journal_line(line: &str) -> Result<OpJournalEvent> {
    if let Ok(event) = serde_json::from_str::<OpJournalEvent>(line) {
        return Ok(event);
    }

    let mut op = serde_json::from_str::<PendingOp>(line)
        .context("line is neither a journal event nor a pending op")?;
    if op.op_id.is_none() {
        op.op_id = Some(op.stable_id());
    }
    Ok(OpJournalEvent::Queued {
        event_id: format!("legacy-{}", op.stable_id()),
        at: op.created_at,
        op,
    })
}

fn journal_segment_name(raw_journal: &str) -> Result<String> {
    let mut ids = Vec::new();
    let mut non_empty = 0usize;

    for (index, line) in raw_journal.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        non_empty += 1;
        match parse_journal_line(trimmed) {
            Ok(event) => ids.push(sanitize_segment_token(event.event_id())),
            Err(_) => ids.push(format!("invalid{}-{}", index + 1, trimmed.len())),
        }
    }

    if non_empty == 0 {
        return Err(anyhow::anyhow!("cannot name empty journal segment"));
    }

    let first = ids.first().cloned().unwrap_or_else(|| "empty".to_string());
    let last = ids.last().cloned().unwrap_or_else(|| "empty".to_string());
    Ok(format!(
        "{:05}-{}-{}.jsonl",
        non_empty,
        shorten_segment_token(&first),
        shorten_segment_token(&last)
    ))
}

fn apply_journal_event(active_ops: &mut BTreeMap<String, PendingOp>, event: OpJournalEvent) {
    match event {
        OpJournalEvent::Queued { mut op, .. } => {
            if op.op_id.is_none() {
                op.op_id = Some(op.stable_id());
            }
            active_ops.insert(op.stable_id(), op);
        }
        OpJournalEvent::Removed { op_id, .. } => {
            active_ops.remove(&op_id);
        }
    }
}

fn sanitize_segment_token(token: &str) -> String {
    token
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn shorten_segment_token(token: &str) -> String {
    token.chars().take(12).collect()
}
