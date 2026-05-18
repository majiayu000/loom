use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{Command, RemoteCommand, WorkspaceCommand};
use crate::commands::remote_status_payload;

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, registry_error, registry_ok,
    run_panel_command, status_for_error_code,
};
use super::super::{PanelState, RemoteSetRequest};

pub(crate) async fn remote_set(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<RemoteSetRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "workspace.remote.set")
    {
        return response;
    }

    let url = req.url.trim().to_string();
    if url.is_empty() {
        let request_id = uuid::Uuid::new_v4().to_string();
        return (
            StatusCode::BAD_REQUEST,
            Json(error_envelope(
                "workspace.remote.set",
                &request_id,
                "ARG_INVALID",
                "remote url is required",
            )),
        );
    }

    run_panel_command(
        &state,
        "workspace.remote.set",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Remote {
                command: RemoteCommand::Set { url },
            },
        },
    )
}

pub(crate) async fn remote_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match remote_status_payload(&state.ctx) {
        Ok((remote, meta)) => (
            StatusCode::OK,
            registry_ok(
                "remote.status",
                json!({"remote": remote, "warnings": meta.warnings}),
            ),
        ),
        Err(err) => (
            status_for_error_code(Some(err.code.as_str())),
            registry_error("remote.status", err.code.as_str(), err.message),
        ),
    }
}
