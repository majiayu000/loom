use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};

use crate::cli::{
    AddArgs, CaptureArgs, Command, ImportObservedArgs, OrphanCleanArgs, ProjectArgs,
    ProjectionMethod, SkillOrphanCommand, TargetCommand, TargetOwnership, WorkspaceBindingCommand,
    WorkspaceCommand,
};

use super::super::auth::{ensure_mutation_authorized, error_envelope, run_panel_command};
use super::super::{
    BindingAddRequest, CaptureRequest, ImportObservedRequest, OrphanCleanRequest, PanelState,
    ProjectRequest, SkillAddRequest, SkillReleaseRequest, SkillRollbackRequest, SkillSaveRequest,
    TargetAddRequest,
};

fn policy_profile_looks_sane(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub(in crate::panel) async fn registry_target_add(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<TargetAddRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "target.add") {
        return response;
    }
    run_panel_command(
        &state,
        "target.add",
        StatusCode::CREATED,
        Command::Target {
            command: TargetCommand::Add(crate::cli::TargetAddArgs {
                agent: req.agent,
                path: req.path,
                ownership: req.ownership.unwrap_or(TargetOwnership::Observed),
            }),
        },
    )
}

pub(in crate::panel) async fn registry_target_remove(
    AxumPath(target_id): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "target.remove") {
        return response;
    }
    run_panel_command(
        &state,
        "target.remove",
        StatusCode::OK,
        Command::Target {
            command: TargetCommand::Remove(crate::cli::TargetShowArgs { target_id }),
        },
    )
}

pub(in crate::panel) async fn registry_binding_add(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<BindingAddRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "workspace.binding.add")
    {
        return response;
    }
    let policy_profile = req
        .policy_profile
        .unwrap_or_else(|| "safe-capture".to_string());
    if !policy_profile_looks_sane(&policy_profile) {
        let request_id = uuid::Uuid::new_v4().to_string();
        return (
            StatusCode::BAD_REQUEST,
            Json(error_envelope(
                "workspace.binding.add",
                &request_id,
                "ARG_INVALID",
                "policy_profile must match [a-z0-9_-]{1,64}",
            )),
        );
    }
    run_panel_command(
        &state,
        "workspace.binding.add",
        StatusCode::CREATED,
        Command::Workspace {
            command: WorkspaceCommand::Binding {
                command: WorkspaceBindingCommand::Add(crate::cli::BindingAddArgs {
                    agent: req.agent,
                    profile: req.profile,
                    matcher_kind: req.matcher_kind,
                    matcher_value: req.matcher_value,
                    target: req.target,
                    policy_profile,
                }),
            },
        },
    )
}

pub(in crate::panel) async fn registry_binding_remove(
    AxumPath(binding_id): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "workspace.binding.remove")
    {
        return response;
    }
    run_panel_command(
        &state,
        "workspace.binding.remove",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Binding {
                command: WorkspaceBindingCommand::Remove(crate::cli::BindingShowArgs {
                    binding_id,
                }),
            },
        },
    )
}

pub(in crate::panel) async fn registry_project(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<ProjectRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.project") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.project",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Project(ProjectArgs {
                skill: req.skill,
                binding: req.binding,
                target: req.target,
                method: req.method.unwrap_or(ProjectionMethod::Symlink),
                dry_run: false,
            }),
        },
    )
}

pub(in crate::panel) async fn registry_skill_add(
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

pub(in crate::panel) async fn registry_skill_import_observed(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<ImportObservedRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "skill.import_observed")
    {
        return response;
    }
    run_panel_command(
        &state,
        "skill.import_observed",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::ImportObserved(ImportObservedArgs {
                target: req.target,
            }),
        },
    )
}

pub(in crate::panel) async fn registry_skill_save(
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

pub(in crate::panel) async fn registry_skill_snapshot(
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

pub(in crate::panel) async fn registry_skill_release(
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

pub(in crate::panel) async fn registry_skill_rollback(
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
                dry_run: false,
            }),
        },
    )
}

pub(in crate::panel) async fn registry_capture(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<CaptureRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.capture") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.capture",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Capture(CaptureArgs {
                skill: req.skill,
                binding: req.binding,
                instance: req.instance,
                message: req.message,
                dry_run: false,
            }),
        },
    )
}

pub(in crate::panel) async fn registry_orphan_clean(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<OrphanCleanRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.orphan.clean")
    {
        return response;
    }
    run_panel_command(
        &state,
        "skill.orphan.clean",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Orphan {
                command: SkillOrphanCommand::Clean(OrphanCleanArgs {
                    delete_live_paths: req.delete_live_paths,
                    dry_run: false,
                }),
            },
        },
    )
}
