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
use skill_diff::registry_skill_diff;
use skill_history::registry_skill_history;
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
pub(super) struct SkillAddRequest {
    pub(super) source: String,
    pub(super) name: String,
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
        .route("/api/registry/status", get(registry_status))
        .route("/api/registry/ops", get(registry_ops))
        .route("/api/registry/ops/diagnose", get(registry_ops_diagnose))
        .route("/api/registry/projections", get(registry_projections))
        .route("/api/registry/bindings", get(registry_bindings))
        .route(
            "/api/registry/bindings/{binding_id}",
            get(registry_binding_show),
        )
        .route("/api/registry/targets", get(registry_targets))
        .route(
            "/api/registry/targets/{target_id}",
            get(registry_target_show),
        )
        .route("/api/registry/targets", post(registry_target_add))
        .route(
            "/api/registry/targets/{target_id}/remove",
            post(registry_target_remove),
        )
        .route("/api/registry/bindings", post(registry_binding_add))
        .route(
            "/api/registry/bindings/{binding_id}/remove",
            post(registry_binding_remove),
        )
        .route("/api/registry/skills", post(registry_skill_add))
        .route("/api/registry/project", post(registry_project))
        .route("/api/registry/capture", post(registry_capture))
        .route(
            "/api/registry/skills/{skill_name}/diff",
            get(registry_skill_diff),
        )
        .route(
            "/api/registry/skills/{skill_name}/history",
            get(registry_skill_history),
        )
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
