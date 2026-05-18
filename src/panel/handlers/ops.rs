use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{Command, HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand};

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, registry_error, registry_ok, run_panel_command,
};
use super::super::{HistoryRepairRequest, PanelState};

// Ops handlers expose the same pending-queue maintenance as
// `loom ops {retry,purge}`. Keep them separate from sync routes because
// retry returns queue before/after counts, while purge intentionally clears
// the pending queue without touching the durable operations history.

pub(crate) async fn ops_retry(
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

pub(crate) async fn ops_purge(
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

pub(crate) async fn ops_history_repair(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<HistoryRepairRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "ops.history.repair")
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

pub(crate) async fn registry_ops_diagnose(
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    match crate::gitops::history_status(&state.ctx) {
        Ok(report) => registry_ok("registry.ops.diagnose", serde_json::json!(report)),
        Err(err) => registry_error("registry.ops.diagnose", "GIT_ERROR", err.to_string()),
    }
}

pub(crate) async fn pending(
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
