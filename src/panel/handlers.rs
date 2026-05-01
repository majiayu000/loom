use std::net::SocketAddr;

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::json;

use crate::cli::{
    AddArgs, CaptureArgs, Command, HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand,
    ProjectArgs, ProjectionMethod, RemoteCommand, SyncCommand, TargetCommand, TargetOwnership,
    WorkspaceBindingCommand, WorkspaceCommand,
};
use crate::commands::{collect_skill_inventory, redact_sensitive_string, remote_status_payload};
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::RegistryStatePaths;

use super::auth::{
    ensure_mutation_authorized, error_envelope, load_registry_snapshot, registry_error,
    registry_ok, run_panel_command, status_for_error_code, status_for_registry_error_payload,
};
use super::{
    BindingAddRequest, CaptureRequest, HistoryRepairRequest, PanelState, ProjectRequest,
    RemoteSetRequest, SkillAddRequest, TargetAddRequest,
};

/// Accept `[a-z0-9_-]{1,64}` for `policy_profile`. The core CLI path enforces
/// the same shape; keeping the panel check here avoids doing a full command
/// dispatch for obviously malformed requests.
fn policy_profile_looks_sane(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

const DEFAULT_OPS_PAGE_SIZE: usize = 100;
const MAX_OPS_PAGE_SIZE: usize = 250;

#[derive(Debug, Default, Deserialize)]
pub(super) struct ProjectionsQuery {
    #[serde(default)]
    pub(super) health: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct OpsQuery {
    #[serde(default)]
    pub(super) limit: Option<usize>,
    #[serde(default)]
    pub(super) offset: Option<usize>,
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
    let remote_url = redact_sensitive_string(&remote_url);
    let registry_paths = RegistryStatePaths::from_app_context(&state.ctx);

    registry_ok(
        "panel.info",
        json!({
            "root": state.ctx.root.display().to_string(),
            "state_dir": state.ctx.state_dir.display().to_string(),
            "registry_targets_file": registry_paths.targets_file.display().to_string(),
            "claude_dir": target_dirs.claude.display().to_string(),
            "codex_dir": target_dirs.codex.display().to_string(),
            "remote_url": remote_url,
        }),
    )
}

pub(super) async fn skills(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let inventory = collect_skill_inventory(&state.ctx);
    registry_ok(
        "panel.skills",
        json!({
            "skills": inventory.source_skills,
            "backup_skills": inventory.backup_skills,
            "source_dirs": inventory
                .source_dirs
                .iter()
                .map(|path: &std::path::PathBuf| path.display().to_string())
                .collect::<Vec<_>>(),
            "warnings": inventory.warnings
        }),
    )
}

pub(super) async fn registry_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match load_registry_snapshot(&state.ctx, "registry.status") {
        Ok(snapshot) => (
            StatusCode::OK,
            registry_ok("registry.status", snapshot.status_view()),
        ),
        Err(err) => {
            let status = status_for_registry_error_payload(&err.0);
            (status, err)
        }
    }
}

/// Return a bounded, newest-first page of the operations journal
/// (`.loom/registry/operations.jsonl`). History only needs row summaries, so omit
/// per-op payload/effects blobs here and keep the response cost bounded even
/// for long-lived registries.
pub(super) async fn registry_ops(
    Query(query): Query<OpsQuery>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
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
                    json!({
                        "op_id": op.op_id,
                        "intent": op.intent,
                        "status": op.status,
                        "ack": op.ack,
                        "last_error": op.last_error,
                        "created_at": op.created_at,
                        "updated_at": op.updated_at,
                    })
                })
                .collect::<Vec<_>>();

            registry_ok(
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
        Err(err) => err,
    }
}

pub(super) async fn registry_projections(
    Query(query): Query<ProjectionsQuery>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    match load_registry_snapshot(&state.ctx, "registry.projections") {
        Ok(snapshot) => {
            let projections: Vec<_> = snapshot
                .projections
                .projections
                .iter()
                .filter(|p| query.health.as_deref().is_none_or(|h| p.health == h))
                .collect();
            registry_ok(
                "registry.projections",
                json!({
                    "state_model": "registry",
                    "count": projections.len(),
                    "projections": projections,
                }),
            )
        }
        Err(err) => err,
    }
}

pub(super) async fn registry_bindings(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_registry_snapshot(&state.ctx, "registry.bindings") {
        Ok(snapshot) => registry_ok(
            "registry.bindings",
            json!({
                "state_model": "registry",
                "count": snapshot.bindings.bindings.len(),
                "bindings": snapshot.bindings.bindings
            }),
        ),
        Err(err) => err,
    }
}

pub(super) async fn registry_binding_show(
    AxumPath(binding_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_registry_snapshot(&state.ctx, "registry.binding.show") {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let binding = match snapshot.binding(&binding_id).cloned() {
        Some(binding) => binding,
        None => {
            return registry_error(
                "registry.binding.show",
                "BINDING_NOT_FOUND",
                format!("binding '{}' not found", binding_id),
            );
        }
    };

    registry_ok(
        "registry.binding.show",
        json!({
            "state_model": "registry",
            "binding": binding,
            "default_target": snapshot.binding_default_target(&binding),
            "rules": snapshot.binding_rules(&binding.binding_id),
            "projections": snapshot.binding_projections(&binding.binding_id)
        }),
    )
}

pub(super) async fn registry_targets(State(state): State<PanelState>) -> Json<serde_json::Value> {
    match load_registry_snapshot(&state.ctx, "registry.targets") {
        Ok(snapshot) => registry_ok(
            "registry.targets",
            json!({
                "state_model": "registry",
                "count": snapshot.targets.targets.len(),
                "targets": snapshot.targets.targets
            }),
        ),
        Err(err) => err,
    }
}

pub(super) async fn registry_target_show(
    AxumPath(target_id): AxumPath<String>,
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    let snapshot = match load_registry_snapshot(&state.ctx, "registry.target.show") {
        Ok(snapshot) => snapshot,
        Err(err) => return err,
    };
    let target = match snapshot.target(&target_id) {
        Some(target) => target,
        None => {
            return registry_error(
                "registry.target.show",
                "TARGET_NOT_FOUND",
                format!("target '{}' not found", target_id),
            );
        }
    };
    let relations = snapshot.target_relations(&target_id);

    registry_ok(
        "registry.target.show",
        json!({
            "state_model": "registry",
            "target": target,
            "bindings": relations.bindings,
            "rules": relations.rules,
            "projections": relations.projections
        }),
    )
}

pub(super) async fn registry_target_add(
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

pub(super) async fn registry_target_remove(
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

pub(super) async fn registry_binding_add(
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

pub(super) async fn registry_binding_remove(
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

pub(super) async fn registry_project(
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

pub(super) async fn registry_skill_add(
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

pub(super) async fn registry_capture(
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

// Ops handlers expose the same pending-queue maintenance as
// `loom ops {retry,purge}`. Keep them separate from sync routes because
// retry returns queue before/after counts, while purge intentionally clears
// the pending queue without touching the durable operations history.

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

pub(super) async fn remote_set(
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

pub(super) async fn remote_status(
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

pub(super) async fn registry_ops_diagnose(
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
    match crate::gitops::history_status(&state.ctx) {
        Ok(report) => registry_ok("registry.ops.diagnose", serde_json::json!(report)),
        Err(err) => registry_error("registry.ops.diagnose", "GIT_ERROR", err.to_string()),
    }
}

pub(super) async fn pending(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.ctx.read_pending_report() {
        Ok(report) => (
            StatusCode::OK,
            registry_ok(
                "pending.list",
                json!({
                    "count": report.ops.len(),
                    "ops": report.ops,
                    "journal_events": report.journal_events,
                    "history_events": report.history_events,
                    "warnings": report.warnings
                }),
            ),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            registry_error("pending.list", "IO_ERROR", err.to_string()),
        ),
    }
}
