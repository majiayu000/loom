use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{Command, SyncCommand, WorkspaceCommand, WorkspaceInitArgs};
use crate::commands::App;
use crate::envelope::Envelope;
use crate::types::ErrorCode;

use super::super::auth::{
    ensure_mutation_authorized, load_registry_snapshot, run_panel_command,
};
use super::super::{PanelState, WorkspaceInitRequest};
use super::shared::{
    DEFAULT_OPS_PAGE_SIZE, MAX_OPS_PAGE_SIZE, operation_summary, panel_command_envelope,
    panel_v1_ok, panel_v1_registry_error,
};
use super::skill_read::build_skill_read_model;
use super::{OpsQuery, ProjectionsQuery};

pub(crate) async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true, "service": "loom-panel"}))
}

pub(crate) async fn v1_health() -> (StatusCode, Json<serde_json::Value>) {
    panel_v1_ok("panel.health", json!({"service": "loom-panel"}))
}

pub(crate) async fn v1_overview(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("panel.overview", app.cmd_status())
}

pub(crate) async fn v1_workspace_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("workspace.status", app.cmd_status())
}

pub(crate) async fn v1_workspace_init(
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

pub(crate) async fn v1_workspace_doctor(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("workspace.doctor", app.cmd_doctor())
}

pub(crate) async fn v1_sync_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("sync.status", app.cmd_sync(&SyncCommand::Status))
}

pub(crate) async fn v1_registry_ops(
    Query(query): Query<OpsQuery>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
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
                    let summary = operation_summary(op);
                    json!({
                        "op_id": op.op_id,
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
                    })
                })
                .collect::<Vec<_>>();

            panel_v1_ok(
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
        Err(err) => panel_v1_registry_error(err),
    }
}

pub(crate) async fn v1_registry_projections(
    Query(query): Query<ProjectionsQuery>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_registry_snapshot(&state.ctx, "registry.projections") {
        Ok(snapshot) => {
            let projections: Vec<_> = snapshot
                .projections
                .projections
                .iter()
                .filter(|p| query.health.as_deref().is_none_or(|h| p.health == h))
                .collect();
            panel_v1_ok(
                "registry.projections",
                json!({
                    "state_model": "registry",
                    "count": projections.len(),
                    "projections": projections,
                }),
            )
        }
        Err(err) => panel_v1_registry_error(err),
    }
}

pub(crate) async fn v1_registry_bindings(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_registry_snapshot(&state.ctx, "registry.bindings") {
        Ok(snapshot) => panel_v1_ok(
            "registry.bindings",
            json!({
                "state_model": "registry",
                "count": snapshot.bindings.bindings.len(),
                "bindings": snapshot.bindings.bindings
            }),
        ),
        Err(err) => panel_v1_registry_error(err),
    }
}

pub(crate) async fn v1_registry_targets(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_registry_snapshot(&state.ctx, "registry.targets") {
        Ok(snapshot) => panel_v1_ok(
            "registry.targets",
            json!({
                "state_model": "registry",
                "count": snapshot.targets.targets.len(),
                "targets": snapshot.targets.targets
            }),
        ),
        Err(err) => panel_v1_registry_error(err),
    }
}

pub(crate) async fn v1_skills(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match build_skill_read_model(&state) {
        Ok((skills, warnings, registry_available)) => (
            StatusCode::OK,
            Json(json!(Envelope::ok(
                "registry.skills",
                uuid::Uuid::new_v4().to_string(),
                json!({
                    "state_model": "union",
                    "registry_available": registry_available,
                    "count": skills.len(),
                    "skills": skills,
                }),
                crate::envelope::Meta {
                    warnings,
                    ..crate::envelope::Meta::default()
                }
            ))),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!(Envelope::err(
                "registry.skills",
                uuid::Uuid::new_v4().to_string(),
                ErrorCode::InternalError,
                err.to_string(),
                serde_json::Value::Object(Default::default())
            ))),
        ),
    }
}
