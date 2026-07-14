use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::PendingOp;

pub(super) const OPS_SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(super) enum OpJournalEvent {
    Queued {
        event_id: String,
        at: DateTime<Utc>,
        op: PendingOp,
    },
    Audited {
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
    pub(super) fn event_id(&self) -> &str {
        match self {
            Self::Queued { event_id, .. }
            | Self::Audited { event_id, .. }
            | Self::Removed { event_id, .. } => event_id,
        }
    }

    pub(super) fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Queued { at, .. } | Self::Audited { at, .. } | Self::Removed { at, .. } => *at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpsSnapshot {
    pub(super) version: u32,
    pub(super) created_at: DateTime<Utc>,
    pub(super) history_events: usize,
    pub(super) active_ops: Vec<PendingOp>,
}

#[derive(Debug, Clone)]
pub(crate) struct HistoryBodySummary {
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
}

pub(crate) fn synthesize_snapshot_raw_from_segment_bodies(
    segment_bodies: &[String],
) -> Result<String> {
    let mut seen_event_ids = std::collections::BTreeSet::new();
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
        version: OPS_SNAPSHOT_VERSION,
        created_at: Utc::now(),
        history_events: seen_event_ids.len(),
        active_ops: active_ops.into_values().collect(),
    };

    serde_json::to_string_pretty(&snapshot).context("failed to encode synthesized ops snapshot")
}

pub(crate) fn summarize_history_body(raw: &str) -> Result<HistoryBodySummary> {
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

pub(crate) fn history_event_ids(raw: &str) -> Result<BTreeSet<String>> {
    let mut event_ids = BTreeSet::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        event_ids.insert(parse_journal_line(trimmed)?.event_id().to_string());
    }
    Ok(event_ids)
}

pub(super) fn parse_journal_line(line: &str) -> Result<OpJournalEvent> {
    if let Ok(event) = serde_json::from_str::<OpJournalEvent>(line) {
        return Ok(event);
    }

    let mut op = serde_json::from_str::<PendingOp>(line)
        .context("line is neither a journal event nor a legacy operation row")?;
    if op.op_id.is_none() {
        op.op_id = Some(op.stable_id());
    }
    Ok(OpJournalEvent::Queued {
        event_id: format!("legacy-{}", op.stable_id()),
        at: op.created_at,
        op,
    })
}

#[allow(dead_code)]
pub(super) fn journal_segment_name(raw_journal: &str) -> Result<String> {
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
        anyhow::bail!("cannot name empty journal segment");
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

pub(super) fn apply_journal_event(
    active_ops: &mut BTreeMap<String, PendingOp>,
    event: OpJournalEvent,
) {
    match event {
        OpJournalEvent::Queued { mut op, .. } => {
            if op.op_id.is_none() {
                op.op_id = Some(op.stable_id());
            }
            active_ops.insert(op.stable_id(), op);
        }
        OpJournalEvent::Audited { .. } => {}
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
