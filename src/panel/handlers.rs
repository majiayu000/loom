use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use crate::cli::{
    CaptureArgs, Command, HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand, ProjectArgs,
    ProjectionMethod, SyncCommand, TargetCommand, TargetOwnership, WorkspaceBindingCommand,
    WorkspaceCommand,
};
use crate::commands::{collect_skill_inventory, remote_status_payload};
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::V3StatePaths;

use super::auth::{
    ensure_mutation_authorized, error_envelope, load_v3_snapshot, run_panel_command,
    status_for_error_code, status_for_v3_error_payload, v3_error, v3_ok,
};
use super::{
    BindingAddRequest, CaptureRequest, HistoryRepairRequest, PanelState, ProjectRequest,
    TargetAddRequest,
};

/// Accept `[a-z0-9_-]{1,64}` for `policy_profile`. The backend does not
/// maintain a closed whitelist (users may extend profiles over time),
/// but the panel surface should refuse obviously malformed input so the
/// V3 bindings file stays auditable. CLI users may still submit other
/// formats directly via `loom workspace binding add`.
fn policy_profile_looks_sane(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub(super) async fn health() -> Json<serde_json::Value> {
    Json(json!({"ok": true, "service": "loom-panel"}))
}

pub(super) async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let target_dirs = resolve_agent_skill_dirs(&state.ctx.root);
    let remote_url = crate::gitops::remote_url(&state.ctx)
        .ok()
        .flatten()
        .unwrap_or_default();
    let v3_paths = V3StatePaths::from_app_context(&state.ctx);

    Json(json!({
        "root": state.ctx.root.display().to_string(),
        "state_dir": state.ctx.state_dir.display().to_string(),
        "v3_targets_file": v3_paths.targets_file.display().to_string(),
        "claude_dir": target_dirs.claude.display().to_string(),
        "codex_dir": target_dirs.codex.display().to_string(),
        "remote_url": remote_url,
    }))
}

pub(super) async fn workspace_status(
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

pub(super) async fn skills(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let inventory = collect_skill_inventory(&state.ctx);
    Json(json!({
        "skills": inventory.source_skills,
        "backup_skills": inventory.backup_skills,
        "source_dirs": inventory
            .source_dirs
            .iter()
            .map(|path: &std::path::PathBuf| path.display().to_string())
            .collect::<Vec<_>>(),
        "warnings": inventory.warnings
    }))
}

pub(super) async fn v3_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => (StatusCode::OK, v3_ok(snapshot.status_view())),
        Err(err) => {
            let status = status_for_v3_error_payload(&err.0);
            (status, err)
        }
    }
}

pub(super) async fn v3_bindings(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(json!({
            "state_model": "v3",
            "count": snapshot.bindings.bindings.len(),
            "bindings": snapshot.bindings.bindings
        })),
        Err(err) => err,
    }
}

pub(super) async fn v3_binding_show(
    AxumPath(binding_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let binding = match snapshot.binding(&binding_id).cloned() {
        Some(binding) => binding,
        None => {
            return v3_error(
                "BINDING_NOT_FOUND",
                format!("binding '{}' not found", binding_id),
            );
        }
    };

    v3_ok(json!({
        "state_model": "v3",
        "binding": binding,
        "default_target": snapshot.binding_default_target(&binding),
        "rules": snapshot.binding_rules(&binding.binding_id),
        "projections": snapshot.binding_projections(&binding.binding_id)
    }))
}

pub(super) async fn v3_targets(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => v3_ok(json!({
            "state_model": "v3",
            "count": snapshot.targets.targets.len(),
            "targets": snapshot.targets.targets
        })),
        Err(err) => err,
    }
}

pub(super) async fn v3_target_show(
    AxumPath(target_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_v3_snapshot(&state.ctx) {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let target = match snapshot.target(&target_id) {
        Some(target) => target,
        None => {
            return v3_error(
                "TARGET_NOT_FOUND",
                format!("target '{}' not found", target_id),
            );
        }
    };
    let relations = snapshot.target_relations(&target_id);

    v3_ok(json!({
        "state_model": "v3",
        "target": target,
        "bindings": relations.bindings,
        "rules": relations.rules,
        "projections": relations.projections
    }))
}

pub(super) async fn v3_target_add(
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
                ownership: req.ownership.unwrap_or(TargetOwnership::Managed),
            }),
        },
    )
}

pub(super) async fn v3_target_remove(
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

pub(super) async fn v3_binding_add(
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

pub(super) async fn v3_binding_remove(
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

pub(super) async fn v3_project(
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
            }),
        },
    )
}

pub(super) async fn v3_capture(
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
            }),
        },
    )
}

// Sync handlers wrap `App::cmd_sync` one-to-one with the corresponding
// `SyncCommand` variant so the panel exposes the same git-backed flow as
// the `loom sync {push,pull,replay}` CLI. Each route goes through
// `ensure_mutation_authorized` + `run_panel_command`, so the JSON envelope,
// error-code mapping, and audit-log semantics match other mutations.

pub(super) async fn sync_push(
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

pub(super) async fn sync_pull(
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

pub(super) async fn sync_replay(
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

pub(super) async fn remote_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match remote_status_payload(&state.ctx) {
        Ok((remote, meta)) => (
            StatusCode::OK,
            Json(json!({"remote": remote, "warnings": meta.warnings})),
        ),
        Err(err) => (
            status_for_error_code(Some(err.code.as_str())),
            Json(json!({
                "ok": false,
                "error": {
                    "code": err.code.as_str(),
                    "message": err.message,
                }
            })),
        ),
    }
}

pub(super) async fn pending(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match state.ctx.read_pending_report() {
        Ok(report) => Json(json!({
            "count": report.ops.len(),
            "ops": report.ops,
            "journal_events": report.journal_events,
            "history_events": report.history_events,
            "warnings": report.warnings
        })),
        Err(err) => Json(json!({"count": 0, "ops": [], "error": err.to_string()})),
    }
}

pub(super) async fn ops_history_diagnose(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "ops.history.diagnose",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::History {
                command: OpsHistoryCommand::Diagnose,
            },
        },
    )
}

pub(super) async fn ops_retry(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "ops.retry") {
        return response;
    }
    run_panel_command(
        &state,
        "ops.retry",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::Retry,
        },
    )
}

pub(super) async fn ops_purge(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "ops.purge") {
        return response;
    }
    run_panel_command(
        &state,
        "ops.purge",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::Purge,
        },
    )
}

pub(super) async fn ops_history_repair(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State(state): State<PanelState>,
    Json(req): Json<HistoryRepairRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(response) = ensure_mutation_authorized(&state, peer, &headers, "ops.history.repair")
    {
        return response;
    }
    let strategy = match req.strategy.as_str() {
        "local" => HistoryRepairStrategyArg::Local,
        "remote" => HistoryRepairStrategyArg::Remote,
        _ => {
            let request_id = uuid::Uuid::new_v4().to_string();
            return (
                StatusCode::BAD_REQUEST,
                Json(error_envelope(
                    "ops.history.repair",
                    &request_id,
                    "ARG_INVALID",
                    "strategy must be 'local' or 'remote'",
                )),
            );
        }
    };
    run_panel_command(
        &state,
        "ops.history.repair",
        StatusCode::OK,
        Command::Ops {
            command: OpsCommand::History {
                command: OpsHistoryCommand::Repair(crate::cli::HistoryRepairArgs { strategy }),
            },
        },
    )
}
