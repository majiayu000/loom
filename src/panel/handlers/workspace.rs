use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{Command, RemoteCommand, SyncCommand, WorkspaceCommand, WorkspaceInitArgs};
use crate::commands::{App, redact_sensitive_string, remote_status_payload};
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::RegistryStatePaths;

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, registry_error, registry_ok,
    registry_ok_with_warnings, run_panel_command, status_for_error_code,
};
use super::super::{PanelState, RemoteSetRequest, WorkspaceInitRequest};
use super::common::{panel_command_envelope, panel_v1_ok};

pub(in crate::panel) async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true, "service": "loom-panel"}))
}

pub(in crate::panel) async fn v1_health() -> (StatusCode, Json<serde_json::Value>) {
    panel_v1_ok("panel.health", json!({"service": "loom-panel"}))
}

pub(in crate::panel) async fn v1_overview(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("panel.overview", app.cmd_status())
}

pub(in crate::panel) async fn v1_workspace_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "workspace.status",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Status,
        },
    )
}

pub(in crate::panel) async fn v1_workspace_init(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<WorkspaceInitRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "workspace.init") {
        return response;
    }
    run_panel_command(
        &state,
        "workspace.init",
        StatusCode::CREATED,
        Command::Workspace {
            command: WorkspaceCommand::Init(WorkspaceInitArgs {
                scan_existing: req.scan_existing,
            }),
        },
    )
}

pub(in crate::panel) async fn v1_workspace_doctor(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "workspace.doctor",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Doctor,
        },
    )
}

pub(in crate::panel) async fn v1_sync_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "sync.status",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Status,
        },
    )
}

pub(in crate::panel) async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let target_dirs = resolve_agent_skill_dirs(&state.ctx.root);
    let registry_paths = RegistryStatePaths::from_app_context(&state.ctx);

    let mut warnings: Vec<String> = Vec::new();
    let remote_url = match crate::gitops::remote_url(&state.ctx) {
        Ok(Some(url)) => redact_sensitive_string(&url),
        Ok(None) => {
            // `gitops::remote_url` returns `Ok(None)` for both "no remote
            // configured" (exit 2 "No such remote 'origin'") and "not a git
            // repository" (exit 128). Probe with `rev-parse --git-dir` to
            // distinguish the two so a missing or corrupt repository is
            // surfaced as a warning instead of being indistinguishable from
            // an unconfigured remote.
            match crate::gitops::run_git_allow_failure(&state.ctx, &["rev-parse", "--git-dir"]) {
                Ok(probe) if !probe.status.success() => {
                    warnings.push(format!(
                        "git repository not initialized at {}",
                        state.ctx.root.display()
                    ));
                }
                Err(err) => {
                    warnings.push(format!("failed to probe git repository: {err}"));
                }
                Ok(_) => {}
            }
            String::new()
        }
        Err(err) => {
            warnings.push(format!("failed to read git remote url: {err}"));
            String::new()
        }
    };

    registry_ok_with_warnings(
        "panel.info",
        json!({
            "root": state.ctx.root.display().to_string(),
            "state_dir": state.ctx.state_dir.display().to_string(),
            "registry_targets_file": registry_paths.targets_file.display().to_string(),
            "claude_dir": target_dirs.claude.display().to_string(),
            "codex_dir": target_dirs.codex.display().to_string(),
            "agent_dirs": target_dirs
                .all
                .iter()
                .map(|dir| json!({
                    "agent": dir.agent,
                    "env_var": dir.env_var,
                    "path": dir.path.display().to_string()
                }))
                .collect::<Vec<_>>(),
            "remote_url": remote_url,
        }),
        warnings,
    )
}

pub(in crate::panel) async fn sync_push(
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
            command: SyncCommand::Push(crate::cli::SyncPushArgs { dry_run: false }),
        },
    )
}

pub(in crate::panel) async fn sync_pull(
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

pub(in crate::panel) async fn sync_replay(
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

pub(in crate::panel) async fn remote_set(
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

pub(in crate::panel) async fn remote_status(
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
