mod auth;
mod handlers;
mod skill_diff;
mod skill_history;
mod static_serve;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
};
use serde::Deserialize;

use crate::cli::{AgentKind, ProjectionMethod, TargetOwnership, WorkspaceMatcherKind};
use crate::state::AppContext;

use handlers::*;
use skill_diff::v3_skill_diff;
use skill_history::v3_skill_history;
use static_serve::{ensure_panel_dist, frontend_index, frontend_static_asset};

#[derive(Clone)]
pub(crate) struct PanelState {
    pub(crate) ctx: Arc<AppContext>,
    pub(crate) panel_origin: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TargetAddRequest {
    pub(super) agent: AgentKind,
    pub(super) path: String,
    #[serde(default)]
    pub(super) ownership: Option<TargetOwnership>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BindingAddRequest {
    pub(super) agent: AgentKind,
    pub(super) profile: String,
    pub(super) matcher_kind: WorkspaceMatcherKind,
    pub(super) matcher_value: String,
    pub(super) target: String,
    #[serde(default)]
    pub(super) policy_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ProjectRequest {
    pub(super) skill: String,
    pub(super) binding: String,
    #[serde(default)]
    pub(super) target: Option<String>,
    #[serde(default)]
    pub(super) method: Option<ProjectionMethod>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CaptureRequest {
    #[serde(default)]
    pub(super) skill: Option<String>,
    #[serde(default)]
    pub(super) binding: Option<String>,
    #[serde(default)]
    pub(super) instance: Option<String>,
    #[serde(default)]
    pub(super) message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct HistoryRepairRequest {
    pub(super) strategy: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoteSetRequest {
    pub(super) url: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct DiffParams {
    #[serde(default)]
    pub(super) rev_a: Option<String>,
    #[serde(default)]
    pub(super) rev_b: Option<String>,
}

pub async fn run_panel(ctx: AppContext, port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    ensure_panel_dist()?;

    let state = PanelState {
        ctx: Arc::new(ctx),
        panel_origin: format!("http://{}", addr),
    };

    let app = Router::new()
        .route("/", get(frontend_index))
        .route("/api/health", get(health))
        .route("/api/info", get(info))
        .route("/api/skills", get(skills))
        .route("/api/v3/status", get(v3_status))
        .route("/api/v3/ops", get(v3_ops))
        .route("/api/v3/ops/diagnose", get(v3_ops_diagnose))
        .route("/api/v3/projections", get(v3_projections))
        .route("/api/v3/bindings", get(v3_bindings))
        .route("/api/v3/bindings/{binding_id}", get(v3_binding_show))
        .route("/api/v3/targets", get(v3_targets))
        .route("/api/v3/targets/{target_id}", get(v3_target_show))
        .route("/api/v3/targets", post(v3_target_add))
        .route("/api/v3/targets/{target_id}/remove", post(v3_target_remove))
        .route("/api/v3/bindings", post(v3_binding_add))
        .route(
            "/api/v3/bindings/{binding_id}/remove",
            post(v3_binding_remove),
        )
        .route("/api/v3/project", post(v3_project))
        .route("/api/v3/capture", post(v3_capture))
        .route("/api/v3/skills/{skill_name}/diff", get(v3_skill_diff))
        .route("/api/v3/skills/{skill_name}/history", get(v3_skill_history))
        .route("/api/remote/status", get(remote_status))
        .route("/api/remote/set", post(remote_set))
        .route("/api/pending", get(pending))
        .route("/api/ops/retry", post(ops_retry))
        .route("/api/ops/purge", post(ops_purge))
        .route("/api/ops/history/repair", post(ops_history_repair))
        .route("/api/sync/push", post(sync_push))
        .route("/api/sync/pull", post(sync_pull))
        .route("/api/sync/replay", post(sync_replay))
        .route("/{*path}", get(frontend_static_asset))
        .with_state(state);

    println!("panel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
