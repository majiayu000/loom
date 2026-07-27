use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{Command, HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand};
use crate::commands::build_ops_activity;
use crate::envelope::Envelope;
use crate::types::ErrorCode;

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, load_registry_snapshot, registry_error,
    registry_ok, run_panel_command,
};
use super::super::{HistoryRepairRequest, PanelState};
use super::common::{OpsQuery, panel_v1_registry_error};

pub(in crate::panel) async fn v1_registry_ops(
    Query(query): Query<OpsQuery>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_registry_snapshot(&state.ctx, "registry.ops") {
        Ok(snapshot) => {
            match build_ops_activity(&state.ctx, &snapshot, query.limit, query.offset) {
                Ok(page) => (
                    StatusCode::OK,
                    Json(json!(Envelope::ok(
                        "registry.ops",
                        uuid::Uuid::new_v4().to_string(),
                        page.data,
                        crate::envelope::Meta {
                            warnings: page.warnings,
                            ..crate::envelope::Meta::default()
                        }
                    ))),
                ),
                Err(err) => {
                    let request_id = uuid::Uuid::new_v4().to_string();
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!(Envelope::err(
                            "registry.ops",
                            request_id,
                            ErrorCode::IoError,
                            err.to_string(),
                            json!({})
                        ))),
                    )
                }
            }
        }
        Err(err) => panel_v1_registry_error(err),
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

pub(in crate::panel) async fn v1_pending(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.ctx.read_existing_registry_ops_report() {
        Ok(report) => (
            StatusCode::OK,
            registry_ok(
                "operation_backlog.list",
                json!({
                    "count": report.operation_counts.actionable_operations,
                    "ops": report.ops,
                    "journal_events": report.operation_counts.journal_events(),
                    "history_events": report.operation_counts.history_events(),
                    "operation_counts": report.operation_counts,
                    "state_model": "registry"
                }),
            ),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            registry_error("operation_backlog.list", "IO_ERROR", err.to_string()),
        ),
    }
}
