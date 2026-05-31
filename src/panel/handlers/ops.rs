use std::collections::BTreeSet;
use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde_json::json;

use crate::cli::{Command, HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand};
use crate::envelope::Envelope;
use crate::state::OpsAuditOperation;
use crate::state_model::RegistryOperationRecord;
use crate::types::ErrorCode;

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, load_registry_snapshot, registry_error,
    registry_ok, run_panel_command,
};
use super::super::{HistoryRepairRequest, PanelState};
use super::common::{DEFAULT_OPS_PAGE_SIZE, MAX_OPS_PAGE_SIZE, OpsQuery, panel_v1_registry_error};

pub(in crate::panel) async fn v1_registry_ops(
    Query(query): Query<OpsQuery>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_registry_snapshot(&state.ctx, "registry.ops") {
        Ok(snapshot) => {
            let audit_report = match state.ctx.read_ops_audit_report() {
                Ok(report) => report,
                Err(err) => {
                    let request_id = uuid::Uuid::new_v4().to_string();
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!(Envelope::err(
                            "registry.ops",
                            request_id,
                            ErrorCode::IoError,
                            err.to_string(),
                            json!({})
                        ))),
                    );
                }
            };
            let limit = query
                .limit
                .unwrap_or(DEFAULT_OPS_PAGE_SIZE)
                .clamp(1, MAX_OPS_PAGE_SIZE);
            let offset = query.offset.unwrap_or(0);
            let registry_count = snapshot.operations.len();
            let mut rows = snapshot
                .operations
                .iter()
                .map(registry_operation_activity_row)
                .collect::<Vec<_>>();
            let audited_snapshot_tags = audit_report
                .operations
                .iter()
                .filter(|op| op.command == "skill.snapshot")
                .filter_map(|op| json_string_field(&op.details, &["tag"]))
                .collect::<BTreeSet<_>>();
            for op in &audit_report.operations {
                if op.command == "snapshot"
                    && json_string_field(&op.details, &["tag"])
                        .is_some_and(|tag| audited_snapshot_tags.contains(&tag))
                {
                    continue;
                }
                rows.push(audit_operation_activity_row(op));
            }
            rows.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
            let total = rows.len();
            let operations = rows
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|(_, _, row)| row)
                .collect::<Vec<_>>();

            (
                StatusCode::OK,
                Json(json!(Envelope::ok(
                    "registry.ops",
                    uuid::Uuid::new_v4().to_string(),
                    json!({
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
                    crate::envelope::Meta {
                        warnings: audit_report.warnings,
                        ..crate::envelope::Meta::default()
                    }
                ))),
            )
        }
        Err(err) => panel_v1_registry_error(err),
    }
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
        "snapshot" => "skill.snapshot".to_string(),
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

/// Return a bounded, newest-first page of the operations journal
/// (`.loom/registry/operations.jsonl`). History only needs row summaries, so omit
/// per-op payload/effects blobs here and keep the response cost bounded even
/// for long-lived registries.
pub(in crate::panel) async fn registry_ops(
    Query(query): Query<OpsQuery>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    match load_registry_snapshot(&state.ctx, "registry.ops") {
        Ok(snapshot) => {
            let total = snapshot.operations.len();
            let limit = query
                .limit
                .unwrap_or(DEFAULT_OPS_PAGE_SIZE)
                .clamp(1, MAX_OPS_PAGE_SIZE);
            let offset = query.offset.unwrap_or(0);
            let end = total.saturating_sub(offset);
            let start = end.saturating_sub(limit);
            let operations = snapshot.operations[start..end]
                .iter()
                .rev()
                .map(|op| {
                    json!({
                        "op_id": op.op_id,
                        "intent": op.intent,
                        "status": op.status,
                        "ack": op.ack,
                        "last_error": op.last_error,
                        "created_at": op.created_at,
                        "updated_at": op.updated_at,
                    })
                })
                .collect::<Vec<_>>();

            registry_ok(
                "registry.ops",
                json!({
                    "state_model": "registry",
                    "count": total,
                    "loaded_count": operations.len(),
                    "offset": offset,
                    "limit": limit,
                    "has_more": start > 0,
                    "operations": operations,
                    "checkpoint": snapshot.checkpoint,
                }),
            )
        }
        Err(err) => err,
    }
}

pub(in crate::panel) async fn ops_retry(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "ops.retry") {
        return response;
    }
    run_panel_command(
        &state,
        "ops.retry",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::Retry,
        },
    )
}

pub(in crate::panel) async fn ops_purge(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "ops.purge") {
        return response;
    }
    run_panel_command(
        &state,
        "ops.purge",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::Purge,
        },
    )
}

pub(in crate::panel) async fn ops_history_repair(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<HistoryRepairRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "ops.history.repair")
    {
        return response;
    }
    let strategy = match req.strategy.as_str() {
        "local" => HistoryRepairStrategyArg::Local,
        "remote" => HistoryRepairStrategyArg::Remote,
        _ => {
            let request_id = uuid::Uuid::new_v4().to_string();
            return (
                StatusCode::BAD_REQUEST,
                Json(error_envelope(
                    "ops.history.repair",
                    &request_id,
                    "ARG_INVALID",
                    "strategy must be 'local' or 'remote'",
                )),
            );
        }
    };
    run_panel_command(
        &state,
        "ops.history.repair",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::History {
                command: OpsHistoryCommand::Repair(crate::cli::HistoryRepairArgs { strategy }),
            },
        },
    )
}

pub(in crate::panel) async fn registry_ops_diagnose(
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    match crate::gitops::history_status(&state.ctx) {
        Ok(report) => registry_ok("registry.ops.diagnose", serde_json::json!(report)),
        Err(err) => registry_error("registry.ops.diagnose", "GIT_ERROR", err.to_string()),
    }
}

pub(in crate::panel) async fn pending(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.ctx.read_pending_report() {
        Ok(report) => (
            StatusCode::OK,
            registry_ok(
                "pending.list",
                json!({
                    "count": report.ops.len(),
                    "ops": report.ops,
                    "journal_events": report.journal_events,
                    "history_events": report.history_events,
                    "warnings": report.warnings
                }),
            ),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            registry_error("pending.list", "IO_ERROR", err.to_string()),
        ),
    }
}
