use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};

use crate::cli::{AddArgs, Command};

use super::super::auth::{ensure_mutation_authorized, run_panel_command};
use super::super::{PanelState, SkillAddRequest, SkillReleaseRequest, SkillRollbackRequest, SkillSaveRequest};

pub(crate) async fn registry_skill_add(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<SkillAddRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.add") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.add",
        StatusCode::CREATED,
        Command::Skill {
            command: crate::cli::SkillCommand::Add(AddArgs {
                source: req.source,
                name: req.name,
            }),
        },
    )
}

pub(crate) async fn registry_skill_save(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<SkillSaveRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.save") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.save",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Save(crate::cli::SaveArgs {
                skill: skill_name,
                message: req.message,
            }),
        },
    )
}

pub(crate) async fn registry_skill_snapshot(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.snapshot") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.snapshot",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Snapshot(crate::cli::SkillOnlyArgs {
                skill: skill_name,
            }),
        },
    )
}

pub(crate) async fn registry_skill_release(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<SkillReleaseRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.release") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.release",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Release(crate::cli::ReleaseArgs {
                skill: skill_name,
                version: req.version,
            }),
        },
    )
}

pub(crate) async fn registry_skill_rollback(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<SkillRollbackRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.rollback") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.rollback",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Rollback(crate::cli::RollbackArgs {
                skill: skill_name,
                to: req.to,
                steps: req.steps,
            }),
        },
    )
}
