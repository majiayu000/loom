mod auth;
mod handlers;
mod skill_diff;
mod skill_history;
mod static_serve;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use serde::Deserialize;

use crate::cli::{AgentKind, ProjectionMethod, TargetOwnership, UseScope, WorkspaceMatcherKind};
use crate::state::AppContext;

use handlers::*;
use skill_diff::registry_skill_diff;
use skill_history::registry_skill_history;
use static_serve::{ensure_panel_dist, frontend_index, frontend_static_asset};

const MAX_PANEL_BODY_BYTES: usize = 1024 * 1024;

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
pub(super) struct UseRequest {
    pub(super) agents: Vec<AgentKind>,
    #[serde(default)]
    pub(super) scope: Option<UseScope>,
    #[serde(default)]
    pub(super) workspace: Option<PathBuf>,
    #[serde(default)]
    pub(super) profile: Option<String>,
    #[serde(default)]
    pub(super) method: Option<ProjectionMethod>,
    #[serde(default)]
    pub(super) target_root: Option<PathBuf>,
    #[serde(default)]
    pub(super) adopt: bool,
    #[serde(default)]
    pub(super) apply: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillAddRequest {
    pub(super) source: String,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ImportObservedRequest {
    #[serde(default)]
    pub(super) target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillSaveRequest {
    #[serde(default)]
    pub(super) message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillReleaseRequest {
    pub(super) version: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillRollbackRequest {
    #[serde(default)]
    pub(super) to: Option<String>,
    #[serde(default)]
    pub(super) steps: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TrashRestoreRequest {
    pub(super) skill: String,
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
pub(super) struct WorkspaceInitRequest {
    #[serde(default)]
    pub(super) scan_existing: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrphanCleanRequest {
    #[serde(default)]
    pub(super) delete_live_paths: bool,
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

    let app = panel_router(state);

    eprintln!("panel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn panel_router(state: PanelState) -> Router {
    Router::new()
        .route("/", get(frontend_index))
        .route("/api/v1/health", get(v1_health))
        .route("/api/v1/overview", get(v1_overview))
        .route("/api/v1/workspace/status", get(v1_workspace_status))
        .route("/api/v1/workspace/init", post(v1_workspace_init))
        .route("/api/v1/workspace/info", get(v1_info))
        .route("/api/v1/workspace/doctor", get(v1_workspace_doctor))
        .route("/api/v1/workspace/remote", post(remote_set))
        .route("/api/v1/registry/status", get(registry_status))
        .route(
            "/api/v1/targets",
            get(v1_registry_targets).post(registry_target_add),
        )
        .route("/api/v1/targets/{target_id}", get(registry_target_show))
        .route(
            "/api/v1/targets/{target_id}/remove",
            post(registry_target_remove),
        )
        .route(
            "/api/v1/bindings",
            get(v1_registry_bindings).post(registry_binding_add),
        )
        .route("/api/v1/bindings/{binding_id}", get(registry_binding_show))
        .route(
            "/api/v1/bindings/{binding_id}/remove",
            post(registry_binding_remove),
        )
        .route("/api/v1/skills", get(v1_skills).post(registry_skill_add))
        .route("/api/v1/skills/trash", get(v1_skill_trash))
        .route(
            "/api/v1/skills/trash/{trash_id}/restore",
            post(registry_skill_trash_restore),
        )
        .route(
            "/api/v1/skills/trash/{trash_id}/purge",
            post(registry_skill_trash_purge),
        )
        .route(
            "/api/v1/skills/import-observed",
            post(registry_skill_import_observed),
        )
        .route(
            "/api/v1/skills/{skill_name}/diagnose",
            get(v1_skill_diagnose),
        )
        .route("/api/v1/skills/{skill_name}/inspect", get(v1_skill_inspect))
        .route("/api/v1/skills/{skill_name}/use", post(registry_skill_use))
        .route(
            "/api/v1/skills/{skill_name}/history",
            get(registry_skill_history),
        )
        .route("/api/v1/skills/{skill_name}/diff", get(registry_skill_diff))
        .route(
            "/api/v1/skills/{skill_name}/trash",
            post(registry_skill_trash_add),
        )
        .route(
            "/api/v1/skills/{skill_name}/commit",
            post(registry_skill_save),
        )
        .route(
            "/api/v1/skills/{skill_name}/release-anchor",
            post(registry_skill_snapshot),
        )
        .route(
            "/api/v1/skills/{skill_name}/release",
            post(registry_skill_release),
        )
        .route(
            "/api/v1/skills/{skill_name}/rollback",
            post(registry_skill_rollback),
        )
        .route("/api/v1/projections", get(v1_registry_projections))
        .route("/api/v1/projections/project", post(registry_project))
        .route("/api/v1/projections/commit", post(registry_capture))
        .route("/api/v1/orphans/clean", post(registry_orphan_clean))
        .route("/api/v1/ops", get(v1_registry_ops))
        .route("/api/v1/ops/diagnose", get(registry_ops_diagnose))
        .route("/api/v1/ops/pending", get(v1_pending))
        .route("/api/v1/ops/retry", post(ops_retry))
        .route("/api/v1/ops/purge", post(ops_purge))
        .route("/api/v1/ops/history/repair", post(ops_history_repair))
        .route("/api/v1/telemetry/report", get(v1_telemetry_report))
        .route("/api/v1/sync/status", get(v1_sync_status))
        .route("/api/v1/sync/push", post(sync_push))
        .route("/api/v1/sync/pull", post(sync_pull))
        .route("/api/v1/sync/replay", post(sync_replay))
        .route("/{*path}", get(frontend_static_asset))
        .layer(DefaultBodyLimit::max(MAX_PANEL_BODY_BYTES))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::ensure_panel_request_authorized,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests;
