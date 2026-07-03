use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use super::AppContext;
use super::journal::{OpJournalEvent, parse_journal_line};

#[derive(Debug, Clone, Serialize)]
pub struct OpsAuditOperation {
    pub op_id: String,
    pub request_id: String,
    pub command: String,
    pub status: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub details: Value,
}

#[derive(Debug, Clone, Default)]
pub struct OpsAuditReport {
    pub operations: Vec<OpsAuditOperation>,
    pub warnings: Vec<String>,
}

impl AppContext {
    pub(crate) fn read_ops_audit_report_with_history(
        &self,
        history_bodies: Vec<(String, String)>,
        history_warnings: Vec<String>,
    ) -> Result<OpsAuditReport> {
        let mut events = Vec::new();
        let mut seen_event_ids = BTreeSet::new();
        let mut warnings = history_warnings;

        for (path, body) in history_bodies {
            collect_audit_events_from_body(
                &path,
                "loom_history",
                &body,
                &mut seen_event_ids,
                &mut events,
                &mut warnings,
            );
        }

        events.sort_by(|left, right| {
            left.at
                .cmp(&right.at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });

        let mut operations = BTreeMap::new();
        for event in events {
            match event.event {
                OpJournalEvent::Queued { mut op, .. } => {
                    if op.op_id.is_none() {
                        op.op_id = Some(op.stable_id());
                    }
                    let op_id = op.stable_id();
                    operations.insert(
                        op_id.clone(),
                        OpsAuditOperation {
                            op_id,
                            request_id: op.request_id,
                            command: op.command,
                            status: "queued".to_string(),
                            source: event.source,
                            created_at: op.created_at,
                            updated_at: event.at,
                            details: op.details,
                        },
                    );
                }
                OpJournalEvent::Audited { mut op, .. } => {
                    if op.op_id.is_none() {
                        op.op_id = Some(op.stable_id());
                    }
                    let op_id = op.stable_id();
                    operations.insert(
                        op_id.clone(),
                        OpsAuditOperation {
                            op_id,
                            request_id: op.request_id,
                            command: op.command,
                            status: "succeeded".to_string(),
                            source: event.source,
                            created_at: op.created_at,
                            updated_at: event.at,
                            details: op.details,
                        },
                    );
                }
                OpJournalEvent::Removed { op_id, reason, .. } => {
                    if let Some(op) = operations.get_mut(&op_id) {
                        op.status = reason;
                        op.updated_at = event.at;
                    }
                }
            }
        }

        Ok(OpsAuditReport {
            operations: operations.into_values().collect(),
            warnings,
        })
    }
}

struct ParsedAuditEvent {
    event_id: String,
    at: DateTime<Utc>,
    source: String,
    event: OpJournalEvent,
}

fn collect_audit_events_from_body(
    label: &str,
    source: &str,
    body: &str,
    seen_event_ids: &mut BTreeSet<String>,
    events: &mut Vec<ParsedAuditEvent>,
    warnings: &mut Vec<String>,
) {
    for (line_no, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match parse_journal_line(trimmed) {
            Ok(event) => {
                let event_id = event.event_id().to_string();
                if seen_event_ids.insert(event_id.clone()) {
                    events.push(ParsedAuditEvent {
                        event_id,
                        at: event.at(),
                        source: source.to_string(),
                        event,
                    });
                }
            }
            Err(err) => warnings.push(format!(
                "skipped malformed operation audit event at {}:{}: {}",
                label,
                line_no + 1,
                err
            )),
        }
    }
}
