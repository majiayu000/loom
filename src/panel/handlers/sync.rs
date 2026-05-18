use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};

use crate::cli::{Command, SyncCommand};

use super::super::auth::{ensure_mutation_authorized, run_panel_command};
use super::super::PanelState;

// Sync handlers wrap `App::cmd_sync` one-to-one with the corresponding
// `SyncCommand` variant so the panel exposes the same git-backed flow as
// the `loom sync {push,pull,replay}` CLI. Each route goes through
// `ensure_mutation_authorized` + `run_panel_command`, so the JSON envelope,
// error-code mapping, and audit-log semantics match other mutations.

pub(crate) async fn sync_push(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "sync.push") {
        return response;
    }
    run_panel_command(
        &state,
        "sync.push",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Push,
        },
    )
}

pub(crate) async fn sync_pull(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "sync.pull") {
        return response;
    }
    run_panel_command(
        &state,
        "sync.pull",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Pull,
        },
    )
}

pub(crate) async fn sync_replay(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "sync.replay") {
        return response;
    }
    run_panel_command(
        &state,
        "sync.replay",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Replay,
        },
    )
}
