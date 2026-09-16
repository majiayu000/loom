use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use clap::Parser;
use serde_json::Value;
use std::net::SocketAddr;

use super::super::{
    PanelState,
    auth::{ensure_mutation_authorized, error_envelope, run_panel_command},
    automation_request::AutomationRequest,
};
use crate::cli::Cli;

pub(in crate::panel) async fn automation_execute(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(request): Json<AutomationRequest>,
) -> (StatusCode, Json<Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "automation.execute")
    {
        return response;
    }
    let argv = std::iter::once("loom".to_string()).chain(request.argv());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error_envelope(
                    "automation.execute",
                    &uuid::Uuid::new_v4().to_string(),
                    "ARG_INVALID",
                    &err.to_string(),
                )),
            );
        }
    };
    match tokio::task::spawn_blocking(move || {
        run_panel_command(&state, "automation.execute", StatusCode::OK, cli.command)
    })
    .await
    {
        Ok(response) => response,
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(error_envelope(
                "automation.execute",
                &uuid::Uuid::new_v4().to_string(),
                "INTERNAL_ERROR",
                &format!("execution interrupted; inspect operation history before retrying: {err}"),
            )),
        ),
    }
}
