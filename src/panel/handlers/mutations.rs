use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{
    AddArgs, Command, ImportObservedArgs, OrphanCleanArgs, ProjectionMethod, SkillOrphanCommand,
    SkillTrashCommand, TargetCommand, TargetOwnership, TrashAddArgs, TrashPurgeArgs,
    TrashRestoreArgs, UseArgs, WorkspaceBindingCommand, WorkspaceCommand,
};
use crate::commands::CommandFailure;
use crate::core::lifecycle::{
    CommitSourceInput, ReleaseAnchorInput, ReleaseVersionInput, RollbackInput,
};
use crate::core::projection::{CommitProjectionInput, ProjectSkillInput};
use crate::core::vocab::ProjectionMethod as CoreProjectionMethod;
use crate::types::ErrorCode;

use super::super::auth::{
    ensure_mutation_authorized, error_envelope, run_panel_command, run_panel_service,
};
use super::super::{
    BindingAddRequest, CaptureRequest, ImportObservedRequest, OrphanCleanRequest, PanelState,
    ProjectRequest, SkillAddRequest, SkillReleaseRequest, SkillRollbackRequest, SkillSaveRequest,
    TargetAddRequest, TrashRestoreRequest, UseRequest,
};

fn policy_profile_looks_sane(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn panel_service_reused() -> CommandFailure {
    CommandFailure::new(
        ErrorCode::InternalError,
        "panel service callback was invoked more than once",
    )
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
                command: WorkspaceBindingCommand::Remove(crate::cli::BindingRemoveArgs {
                    binding_id,
                    orphan_projections: false,
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
    let input = ProjectSkillInput {
        skill: req.skill,
        binding: req.binding,
        target: req.target,
        method: CoreProjectionMethod::from(req.method.unwrap_or(ProjectionMethod::Symlink)),
    };
    let audit_input = json!({
        "source": "panel",
        "service": "projection.project_skill",
        "input": {
            "skill": input.skill,
            "binding": input.binding,
            "target": input.target,
            "method": format!("{:?}", input.method),
        }
    });
    let ctx = state.ctx.clone();
    let mut input = Some(input);
    let mut service = move |request_id: String| {
        crate::core::projection::project_skill(
            &ctx,
            input.take().ok_or_else(panel_service_reused)?,
            &request_id,
        )
    };
    run_panel_service(
        &state,
        "skill.project",
        StatusCode::OK,
        audit_input,
        &mut service,
    )
}

pub(in crate::panel) async fn registry_skill_use(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<UseRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "use") {
        return response;
    }
    run_panel_command(
        &state,
        "use",
        StatusCode::OK,
        Command::Use(UseArgs {
            skill: skill_name,
            agents: req.agents,
            scope: req.scope.unwrap_or(crate::cli::UseScope::Project),
            workspace: req.workspace,
            profile: req.profile.unwrap_or_else(|| "default".to_string()),
            method: req.method.unwrap_or(ProjectionMethod::Symlink),
            target_root: req.target_root,
            adopt: req.adopt,
            apply: req.apply,
        }),
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
                source_ref: None,
                subdir: None,
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

pub(in crate::panel) async fn registry_skill_trash_add(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.trash.add") {
        return response;
    }
    run_panel_command(
        &state,
        "skill.trash.add",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Trash {
                command: SkillTrashCommand::Add(TrashAddArgs {
                    skill: skill_name,
                    dry_run: false,
                }),
            },
        },
    )
}

pub(in crate::panel) async fn registry_skill_trash_restore(
    AxumPath(trash_id): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<TrashRestoreRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) =
        ensure_mutation_authorized(&state, peer, &headers, "skill.trash.restore")
    {
        return response;
    }
    run_panel_command(
        &state,
        "skill.trash.restore",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Trash {
                command: SkillTrashCommand::Restore(TrashRestoreArgs {
                    skill: req.skill,
                    trash_id: Some(trash_id),
                }),
            },
        },
    )
}

pub(in crate::panel) async fn registry_skill_trash_purge(
    AxumPath(trash_id): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.trash.purge")
    {
        return response;
    }
    run_panel_command(
        &state,
        "skill.trash.purge",
        StatusCode::OK,
        Command::Skill {
            command: crate::cli::SkillCommand::Trash {
                command: SkillTrashCommand::Purge(TrashPurgeArgs {
                    trash_id,
                    dry_run: false,
                }),
            },
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
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.commit") {
        return response;
    }
    let input = CommitSourceInput {
        skill: skill_name,
        message: req.message,
    };
    let audit_input = json!({
        "source": "panel",
        "service": "lifecycle.commit_source",
        "input": {
            "skill": input.skill,
            "message": input.message,
        }
    });
    let ctx = state.ctx.clone();
    let mut input = Some(input);
    let mut service = move |request_id: String| {
        crate::core::lifecycle::commit_source(
            &ctx,
            input.take().ok_or_else(panel_service_reused)?,
            &request_id,
        )
    };
    run_panel_service(
        &state,
        "skill.commit",
        StatusCode::OK,
        audit_input,
        &mut service,
    )
}

pub(in crate::panel) async fn registry_skill_snapshot(
    AxumPath(skill_name): AxumPath<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.release") {
        return response;
    }
    let input = ReleaseAnchorInput { skill: skill_name };
    let audit_input = json!({
        "source": "panel",
        "service": "lifecycle.release_anchor",
        "input": {
            "skill": input.skill,
        }
    });
    let ctx = state.ctx.clone();
    let mut input = Some(input);
    let mut service = move |request_id: String| {
        crate::core::lifecycle::release_anchor(
            &ctx,
            input.take().ok_or_else(panel_service_reused)?,
            &request_id,
        )
    };
    run_panel_service(
        &state,
        "skill.release",
        StatusCode::OK,
        audit_input,
        &mut service,
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
    let input = ReleaseVersionInput {
        skill: skill_name,
        version: req.version,
    };
    let audit_input = json!({
        "source": "panel",
        "service": "lifecycle.release_version",
        "input": {
            "skill": input.skill,
            "version": input.version,
        }
    });
    let ctx = state.ctx.clone();
    let mut input = Some(input);
    let mut service = move |request_id: String| {
        crate::core::lifecycle::release_version(
            &ctx,
            input.take().ok_or_else(panel_service_reused)?,
            &request_id,
        )
    };
    run_panel_service(
        &state,
        "skill.release",
        StatusCode::OK,
        audit_input,
        &mut service,
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
    let input = RollbackInput {
        skill: skill_name,
        to: req.to,
        steps: req.steps,
    };
    let audit_input = json!({
        "source": "panel",
        "service": "lifecycle.rollback",
        "input": {
            "skill": input.skill,
            "to": input.to,
            "steps": input.steps,
        }
    });
    let ctx = state.ctx.clone();
    let mut input = Some(input);
    let mut service = move |request_id: String| {
        crate::core::lifecycle::rollback(
            &ctx,
            input.take().ok_or_else(panel_service_reused)?,
            &request_id,
        )
    };
    run_panel_service(
        &state,
        "skill.rollback",
        StatusCode::OK,
        audit_input,
        &mut service,
    )
}

pub(in crate::panel) async fn registry_capture(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<CaptureRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "skill.commit") {
        return response;
    }
    let input = CommitProjectionInput {
        skill: req.skill.unwrap_or_default(),
        binding: req.binding,
        instance: req.instance,
        message: req.message,
    };
    let audit_input = json!({
        "source": "panel",
        "service": "projection.commit_projection",
        "input": {
            "skill": input.skill,
            "binding": input.binding,
            "instance": input.instance,
            "message": input.message,
        }
    });
    let ctx = state.ctx.clone();
    let mut input = Some(input);
    let mut service = move |request_id: String| {
        crate::core::projection::commit_projection(
            &ctx,
            input.take().ok_or_else(panel_service_reused)?,
            &request_id,
        )
    };
    run_panel_service(
        &state,
        "skill.commit",
        StatusCode::OK,
        audit_input,
        &mut service,
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
