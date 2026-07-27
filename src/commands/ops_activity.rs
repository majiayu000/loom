//! Shared ops activity read model.
//!
//! Merges replayable registry operations with the loom-history audit journal
//! into a single newest-first activity feed. Used by both the panel
//! `/api/registry/ops` endpoint and the CLI `ops list --activity` surface so
//! the two transports expose the same data.

use std::collections::BTreeSet;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::json;

use crate::gitops;
use crate::state::{AppContext, OpsAuditOperation};
use crate::state_model::{RegistryOperationRecord, RegistrySnapshot};

pub(crate) const DEFAULT_OPS_ACTIVITY_PAGE_SIZE: usize = 100;
pub(crate) const MAX_OPS_ACTIVITY_PAGE_SIZE: usize = 250;

/// One page of the merged ops activity feed plus non-fatal warnings.
pub(crate) struct OpsActivityPage {
    pub(crate) data: serde_json::Value,
    pub(crate) warnings: Vec<String>,
}

/// Build the merged registry + audit activity read model for one page.
///
/// `limit` is clamped to `1..=MAX_OPS_ACTIVITY_PAGE_SIZE` and defaults to
/// `DEFAULT_OPS_ACTIVITY_PAGE_SIZE`; `offset` defaults to zero.
pub(crate) fn build_ops_activity(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<OpsActivityPage> {
    let (history_bodies, history_warnings) = match gitops::history_journal_bodies(ctx) {
        Ok(bodies) => (bodies, Vec::new()),
        Err(err) => (
            Vec::new(),
            vec![format!("failed to read loom-history branch: {}", err)],
        ),
    };
    let audit_report = ctx.read_ops_audit_report_with_history(history_bodies, history_warnings)?;
    let limit = limit
        .unwrap_or(DEFAULT_OPS_ACTIVITY_PAGE_SIZE)
        .clamp(1, MAX_OPS_ACTIVITY_PAGE_SIZE);
    let offset = offset.unwrap_or(0);
    let registry_count = snapshot.operations.len();
    let rows = merge_activity_rows(&snapshot.operations, &audit_report.operations);
    let total = rows.len();
    let operations = rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, _, row)| row)
        .collect::<Vec<_>>();
    Ok(OpsActivityPage {
        data: json!({
            "state_model": "registry",
            "count": total,
            "registry_count": registry_count,
            "audit_count": audit_report.operations.len(),
            "loaded_count": operations.len(),
            "offset": offset,
            "limit": limit,
            "has_more": offset + operations.len() < total,
            "operations": operations,
            "checkpoint": snapshot.checkpoint,
        }),
        warnings: audit_report.warnings,
    })
}

/// Merge registry operation records with audit operations into activity rows
/// sorted newest-first (by `updated_at`, then id, both descending).
///
/// Audit `release` operations whose snapshot tag is already covered by a
/// `skill.release` audit entry are dropped so a release does not appear twice.
fn merge_activity_rows(
    registry_ops: &[RegistryOperationRecord],
    audit_ops: &[OpsAuditOperation],
) -> Vec<(DateTime<Utc>, String, serde_json::Value)> {
    let mut rows = registry_ops
        .iter()
        .map(registry_operation_activity_row)
        .collect::<Vec<_>>();
    let audited_snapshot_tags = audit_ops
        .iter()
        .filter(|op| op.command == "skill.release")
        .filter_map(|op| json_string_field(&op.details, &["tag"]))
        .collect::<BTreeSet<_>>();
    for op in audit_ops {
        if op.command == "release"
            && json_string_field(&op.details, &["tag"])
                .is_some_and(|tag| audited_snapshot_tags.contains(&tag))
        {
            continue;
        }
        rows.push(audit_operation_activity_row(op));
    }
    rows.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    rows
}

fn registry_operation_activity_row(
    op: &RegistryOperationRecord,
) -> (DateTime<Utc>, String, serde_json::Value) {
    let summary = operation_summary(op);
    (
        op.updated_at,
        op.op_id.clone(),
        json!({
            "op_id": op.op_id,
            "audit_id": null,
            "source": "registry",
            "intent": op.intent,
            "status": op.status,
            "ack": op.ack,
            "request_id": summary.request_id,
            "skill": summary.skill,
            "target": summary.target,
            "binding": summary.binding,
            "method": summary.method,
            "last_error": op.last_error,
            "created_at": op.created_at,
            "updated_at": op.updated_at,
        }),
    )
}

fn audit_operation_activity_row(
    op: &OpsAuditOperation,
) -> (DateTime<Utc>, String, serde_json::Value) {
    let intent = audit_operation_intent(op);
    let summary = audit_operation_summary(op);
    let ack = matches!(op.status.as_str(), "acked" | "purged" | "succeeded");
    (
        op.updated_at,
        op.op_id.clone(),
        json!({
            "op_id": null,
            "audit_id": op.op_id,
            "source": op.source,
            "intent": intent,
            "status": op.status,
            "ack": ack,
            "request_id": op.request_id,
            "skill": summary.skill,
            "target": summary.target,
            "binding": summary.binding,
            "method": summary.method,
            "last_error": null,
            "created_at": op.created_at,
            "updated_at": op.updated_at,
        }),
    )
}

fn audit_operation_intent(op: &OpsAuditOperation) -> String {
    match op.command.as_str() {
        "release" => "skill.release".to_string(),
        other => other.to_string(),
    }
}

fn audit_operation_summary(op: &OpsAuditOperation) -> OperationSummary {
    OperationSummary {
        request_id: Some(op.request_id.clone()),
        skill: json_string_field(&op.details, &["skill_id", "skill"]),
        target: json_string_field(&op.details, &["target_id", "target"]),
        binding: json_string_field(&op.details, &["binding_id", "binding"]),
        method: json_string_field(&op.details, &["method"]),
    }
}

#[derive(Default)]
struct OperationSummary {
    request_id: Option<String>,
    skill: Option<String>,
    target: Option<String>,
    binding: Option<String>,
    method: Option<String>,
}

fn operation_summary(op: &RegistryOperationRecord) -> OperationSummary {
    OperationSummary {
        request_id: json_string_field(&op.payload, &["request_id"]),
        skill: operation_skill_summary(op),
        target: json_string_field(&op.payload, &["target_id", "target"]),
        binding: json_string_field(&op.payload, &["binding_id", "binding"]),
        method: json_string_field(&op.payload, &["method"]),
    }
}

fn operation_skill_summary(op: &RegistryOperationRecord) -> Option<String> {
    if let Some(skill) = json_string_field(&op.payload, &["skill_id", "skill"]) {
        return Some(skill);
    }
    for field in ["imported", "updated"] {
        let skills = op
            .effects
            .get(field)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("skill").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        if !skills.is_empty() {
            return Some(skills.join(", "));
        }
    }
    None
}

fn json_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
    }

    fn registry_op(op_id: &str, seconds: i64) -> RegistryOperationRecord {
        RegistryOperationRecord {
            op_id: op_id.to_string(),
            intent: "skill.project".to_string(),
            status: "succeeded".to_string(),
            ack: false,
            payload: json!({
                "skill_id": "demo-skill",
                "binding_id": "binding-1",
                "target_id": "target-1",
                "method": "copy",
                "request_id": "req-1"
            }),
            effects: json!({}),
            last_error: None,
            created_at: at(seconds),
            updated_at: at(seconds),
        }
    }

    fn audit_op(
        op_id: &str,
        command: &str,
        details: serde_json::Value,
        seconds: i64,
    ) -> OpsAuditOperation {
        OpsAuditOperation {
            op_id: op_id.to_string(),
            request_id: format!("req-{op_id}"),
            command: command.to_string(),
            status: "succeeded".to_string(),
            source: "loom_history".to_string(),
            created_at: at(seconds),
            updated_at: at(seconds),
            details,
        }
    }

    #[test]
    fn merge_sorts_rows_newest_first() {
        let registry = vec![registry_op("op-a", 10), registry_op("op-b", 30)];
        let audit = vec![audit_op("audit-a", "snapshot", json!({}), 20)];

        let rows = merge_activity_rows(&registry, &audit);

        let ids = rows
            .iter()
            .map(|(_, id, _)| id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["op-b", "audit-a", "op-a"]);
    }

    #[test]
    fn merge_drops_release_audit_rows_covered_by_skill_release_tag() {
        let tag = "snapshot/demo-skill/20260518T000000Z-deadbee";
        let audit = vec![
            audit_op("audit-release", "skill.release", json!({ "tag": tag }), 10),
            audit_op("audit-legacy", "release", json!({ "tag": tag }), 20),
            audit_op(
                "audit-other",
                "release",
                json!({ "tag": "snapshot/other/20260518T000000Z-0000000" }),
                30,
            ),
        ];

        let rows = merge_activity_rows(&[], &audit);

        let ids = rows
            .iter()
            .map(|(_, id, _)| id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["audit-other", "audit-release"]);
        assert_eq!(rows[0].2["intent"], json!("skill.release"));
    }

    #[test]
    fn registry_row_carries_summary_fields_without_raw_payload() {
        let rows = merge_activity_rows(&[registry_op("op-a", 0)], &[]);

        let row = &rows[0].2;
        assert_eq!(row["op_id"], json!("op-a"));
        assert_eq!(row["audit_id"], serde_json::Value::Null);
        assert_eq!(row["source"], json!("registry"));
        assert_eq!(row["skill"], json!("demo-skill"));
        assert_eq!(row["binding"], json!("binding-1"));
        assert_eq!(row["target"], json!("target-1"));
        assert_eq!(row["method"], json!("copy"));
        assert_eq!(row["request_id"], json!("req-1"));
        assert!(row.get("payload").is_none());
        assert!(row.get("effects").is_none());
    }

    #[test]
    fn registry_row_summarizes_imported_skills_from_effects() {
        let mut op = registry_op("op-import", 0);
        op.payload = json!({});
        op.effects = json!({
            "imported": [{ "skill": "alpha" }, { "skill": "beta" }]
        });

        let rows = merge_activity_rows(&[op], &[]);

        assert_eq!(rows[0].2["skill"], json!("alpha, beta"));
    }

    #[test]
    fn audit_row_maps_ack_from_status() {
        let mut acked = audit_op("audit-acked", "snapshot", json!({}), 0);
        acked.status = "acked".to_string();
        let mut queued = audit_op("audit-queued", "snapshot", json!({}), 1);
        queued.status = "queued".to_string();

        let rows = merge_activity_rows(&[], &[acked, queued]);

        assert_eq!(rows[0].2["audit_id"], json!("audit-queued"));
        assert_eq!(rows[0].2["ack"], json!(false));
        assert_eq!(rows[1].2["audit_id"], json!("audit-acked"));
        assert_eq!(rows[1].2["ack"], json!(true));
    }
}
