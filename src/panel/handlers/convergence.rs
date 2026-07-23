use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{ApplyArgs, Command, PlanCommand, PlanConvergeArgs};

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, run_panel_command, run_panel_service,
};
use super::super::{ConvergenceApplyRequest, ConvergencePlanRequest, PanelState};

pub(in crate::panel) async fn registry_convergence_plan(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<ConvergencePlanRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "plan.converge") {
        return response;
    }
    if req.accept_restart_required && !req.require_runtime {
        let request_id = uuid::Uuid::new_v4().to_string();
        return (
            StatusCode::BAD_REQUEST,
            Json(error_envelope(
                "plan.converge",
                &request_id,
                "ARG_INVALID",
                "accept_restart_required requires require_runtime",
            )),
        );
    }
    run_panel_command(
        &state,
        "plan.converge",
        StatusCode::CREATED,
        Command::Plan {
            command: PlanCommand::Converge(PlanConvergeArgs {
                skill: skill_name,
                from_source: true,
                from_projection: false,
                instance: None,
                agent: req.agent,
                workspace: req.workspace,
                profile: req.profile,
                require_runtime: req.require_runtime,
                accept_restart_required: req.accept_restart_required,
                push_remote: req.push_remote,
            }),
        },
    )
}

pub(in crate::panel) async fn registry_convergence_apply(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<ConvergenceApplyRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "apply") {
        return response;
    }
    let args = ApplyArgs {
        plan_id: req.plan_id,
        plan_digest: Some(req.plan_digest),
        idempotency_key: req.idempotency_key,
        approvals: req.approvals,
    };
    let audit_input = json!({
        "source": "panel",
        "service": "convergence.apply",
        "input": &args,
    });
    let app = crate::commands::App {
        ctx: (*state.ctx).clone(),
    };
    let mut service = move |request_id: String| app.cmd_apply_convergence(&args, &request_id);
    run_panel_service(&state, "apply", StatusCode::OK, audit_input, &mut service)
}
