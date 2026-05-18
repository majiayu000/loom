use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::json;

use crate::cli::{
    AddArgs, CaptureArgs, Command, HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand,
    OrphanCleanArgs, ProjectArgs, ProjectionMethod, RemoteCommand, SkillOrphanCommand, SyncCommand,
    TargetCommand, TargetOwnership, WorkspaceBindingCommand, WorkspaceCommand, WorkspaceInitArgs,
};
use crate::commands::{
    App, CommandFailure, collect_skill_inventory, redact_sensitive_string, remote_status_payload,
};
use crate::envelope::Envelope;
use crate::gitops;
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::{RegistryOperationRecord, RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::auth::{
    ensure_mutation_authorized, error_envelope, load_registry_snapshot, registry_error,
    registry_ok, registry_ok_with_warnings, run_panel_command, status_for_error_code,
    status_for_registry_error_payload,
};
use super::{
    BindingAddRequest, CaptureRequest, HistoryRepairRequest, OrphanCleanRequest, PanelState,
    ProjectRequest, RemoteSetRequest, SkillAddRequest, SkillReleaseRequest, SkillRollbackRequest,
    SkillSaveRequest, TargetAddRequest, WorkspaceInitRequest,
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

#[derive(Debug, Default)]
struct SkillReadRow {
    skill_id: String,
    source_path: Option<PathBuf>,
    source_status: Option<&'static str>,
    sources: BTreeSet<&'static str>,
    binding_ids: BTreeSet<String>,
    target_ids: BTreeSet<String>,
    projection_count: usize,
    latest_rev: Option<String>,
    latest_updated_at: Option<String>,
    release_tags: Vec<String>,
    snapshot_tags: Vec<String>,
    observed_imported: bool,
}

impl SkillReadRow {
    fn new(skill_id: String) -> Self {
        Self {
            skill_id,
            ..Self::default()
        }
    }
}

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

pub(super) async fn v1_health() -> (StatusCode, Json<serde_json::Value>) {
    panel_v1_ok("panel.health", json!({"service": "loom-panel"}))
}

pub(super) async fn v1_overview(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("panel.overview", app.cmd_status())
}

pub(super) async fn v1_workspace_status(
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

pub(super) async fn v1_workspace_init(
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

pub(super) async fn v1_workspace_doctor(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "workspace.doctor",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Doctor,
        },
    )
}

pub(super) async fn v1_sync_status(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "sync.status",
        StatusCode::OK,
        Command::Sync {
            command: SyncCommand::Status,
        },
    )
}

pub(super) async fn v1_registry_ops(
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

#[derive(Default)]
struct OperationSummary {
    request_id: Option<String>,
    skill: Option<String>,
    target: Option<String>,
    binding: Option<String>,
    method: Option<String>,
}

fn operation_summary(op: &RegistryOperationRecord) -> OperationSummary {
    OperationSummary {
        request_id: json_string_field(&op.payload, &["request_id"]),
        skill: operation_skill_summary(op),
        target: json_string_field(&op.payload, &["target_id", "target"]),
        binding: json_string_field(&op.payload, &["binding_id", "binding"]),
        method: json_string_field(&op.payload, &["method"]),
    }
}

fn operation_skill_summary(op: &RegistryOperationRecord) -> Option<String> {
    if let Some(skill) = json_string_field(&op.payload, &["skill_id", "skill"]) {
        return Some(skill);
    }
    for field in ["imported", "updated"] {
        let skills = op
            .effects
            .get(field)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("skill").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>();
        if !skills.is_empty() {
            return Some(skills.join(", "));
        }
    }
    None
}

fn json_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

pub(super) async fn v1_registry_projections(
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

pub(super) async fn v1_registry_bindings(
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

pub(super) async fn v1_registry_targets(
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

pub(super) async fn v1_skills(
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

fn build_skill_read_model(
    state: &PanelState,
) -> anyhow::Result<(Vec<serde_json::Value>, Vec<String>, bool)> {
    let mut warnings = Vec::new();
    let mut rows: BTreeMap<String, SkillReadRow> = BTreeMap::new();

    add_source_skill_rows(&state.ctx.skills_dir, &mut rows)?;

    let paths = RegistryStatePaths::from_app_context(&state.ctx);
    let snapshot = paths.maybe_load_snapshot()?;
    let registry_available = snapshot.is_some();
    if let Some(snapshot) = snapshot.as_ref() {
        add_registry_skill_rows(snapshot, &mut rows);
        add_observed_import_rows(snapshot, &mut rows);
    } else {
        warnings.push(format!(
            "registry state not initialized under {}",
            paths.registry_dir.display()
        ));
    }

    add_skill_tags(state, &mut rows, &mut warnings)?;

    Ok((
        rows.into_values()
            .map(skill_row_to_json)
            .collect::<Vec<_>>(),
        warnings,
        registry_available,
    ))
}

fn skill_row<'a>(
    rows: &'a mut BTreeMap<String, SkillReadRow>,
    skill_id: &str,
) -> &'a mut SkillReadRow {
    rows.entry(skill_id.to_string())
        .or_insert_with(|| SkillReadRow::new(skill_id.to_string()))
}

fn add_source_skill_rows(
    skills_dir: &Path,
    rows: &mut BTreeMap<String, SkillReadRow>,
) -> anyhow::Result<()> {
    if !skills_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(skills_dir)? {
        let entry = entry?;
        let skill_id = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let row = skill_row(rows, &skill_id);
        row.sources.insert("source");
        row.source_path = Some(path.clone());
        row.source_status = Some(if path.is_dir() && path.join("SKILL.md").is_file() {
            "present"
        } else {
            "non-compliant"
        });
    }
    Ok(())
}

fn add_registry_skill_rows(snapshot: &RegistrySnapshot, rows: &mut BTreeMap<String, SkillReadRow>) {
    for rule in &snapshot.rules.rules {
        let row = skill_row(rows, &rule.skill_id);
        row.sources.insert("rule");
        row.binding_ids.insert(rule.binding_id.clone());
        row.target_ids.insert(rule.target_id.clone());
    }

    for projection in &snapshot.projections.projections {
        let row = skill_row(rows, &projection.skill_id);
        row.sources.insert("projection");
        if let Some(binding_id) = projection.binding_id.as_ref() {
            row.binding_ids.insert(binding_id.clone());
        }
        row.target_ids.insert(projection.target_id.clone());
        row.projection_count += 1;
        if !projection.last_applied_rev.is_empty()
            && row.latest_rev.is_none()
            && projection.updated_at.is_none()
        {
            row.latest_rev = Some(projection.last_applied_rev.clone());
        }
        if let Some(updated_at) = projection.updated_at {
            let updated_at = updated_at.to_rfc3339();
            if row
                .latest_updated_at
                .as_ref()
                .is_none_or(|current| updated_at > *current)
            {
                row.latest_updated_at = Some(updated_at);
                row.latest_rev = Some(projection.last_applied_rev.clone());
            }
        }
    }
}

fn add_observed_import_rows(
    snapshot: &RegistrySnapshot,
    rows: &mut BTreeMap<String, SkillReadRow>,
) {
    for op in &snapshot.operations {
        if op.intent != "skill.import_observed" && op.intent != "skill.monitor_observed" {
            continue;
        }
        for field in ["imported", "updated"] {
            if let Some(items) = op.effects.get(field).and_then(serde_json::Value::as_array) {
                for item in items {
                    if let Some(skill_id) = item.get("skill").and_then(serde_json::Value::as_str) {
                        let row = skill_row(rows, skill_id);
                        row.sources.insert("observed");
                        row.observed_imported = true;
                    }
                }
            }
        }
    }
}

fn add_skill_tags(
    state: &PanelState,
    rows: &mut BTreeMap<String, SkillReadRow>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !gitops::repo_is_initialized(&state.ctx)? {
        warnings.push(
            "git repository not initialized; release and snapshot tags unavailable".to_string(),
        );
        return Ok(());
    }

    let output = gitops::run_git_allow_failure(&state.ctx, &["tag", "--list"])?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "failed to list git tags: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    for tag in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(rest) = tag.strip_prefix("release/") {
            if let Some((skill_id, version)) = rest.split_once('/') {
                let row = skill_row(rows, skill_id);
                row.sources.insert("release_tag");
                row.release_tags.push(version.to_string());
            }
        } else if let Some(rest) = tag.strip_prefix("snapshot/")
            && let Some((skill_id, snapshot)) = rest.split_once('/')
        {
            let row = skill_row(rows, skill_id);
            row.sources.insert("snapshot_tag");
            row.snapshot_tags.push(snapshot.to_string());
        }
    }
    Ok(())
}

fn skill_row_to_json(row: SkillReadRow) -> serde_json::Value {
    let source_status = row.source_status.unwrap_or("missing");
    json!({
        "skill_id": row.skill_id,
        "source_status": source_status,
        "source_path": row.source_path.map(|path| path.display().to_string()),
        "latest_rev": row.latest_rev,
        "latest_updated_at": row.latest_updated_at,
        "bindings_count": row.binding_ids.len(),
        "projections_count": row.projection_count,
        "target_ids": row.target_ids.into_iter().collect::<Vec<_>>(),
        "release_tags": row.release_tags,
        "snapshot_tags": row.snapshot_tags,
        "observed_imported": row.observed_imported,
        "sources": row.sources.into_iter().collect::<Vec<_>>(),
    })
}

pub(super) async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let target_dirs = resolve_agent_skill_dirs(&state.ctx.root);
    let registry_paths = RegistryStatePaths::from_app_context(&state.ctx);

    let mut warnings: Vec<String> = Vec::new();
    let remote_url = match crate::gitops::remote_url(&state.ctx) {
        Ok(Some(url)) => redact_sensitive_string(&url),
        Ok(None) => {
            // `gitops::remote_url` returns `Ok(None)` for both "no remote
            // configured" (exit 2 "No such remote 'origin'") and "not a git
            // repository" (exit 128). Probe with `rev-parse --git-dir` to
            // distinguish the two so a missing or corrupt repository is
            // surfaced as a warning instead of being indistinguishable from
            // an unconfigured remote.
            match crate::gitops::run_git_allow_failure(&state.ctx, &["rev-parse", "--git-dir"]) {
                Ok(probe) if !probe.status.success() => {
                    warnings.push(format!(
                        "git repository not initialized at {}",
                        state.ctx.root.display()
                    ));
                }
                Err(err) => {
                    warnings.push(format!("failed to probe git repository: {err}"));
                }
                Ok(_) => {}
            }
            String::new()
        }
        Err(err) => {
            warnings.push(format!("failed to read git remote url: {err}"));
            String::new()
        }
    };

    registry_ok_with_warnings(
        "panel.info",
        json!({
            "root": state.ctx.root.display().to_string(),
            "state_dir": state.ctx.state_dir.display().to_string(),
            "registry_targets_file": registry_paths.targets_file.display().to_string(),
            "claude_dir": target_dirs.claude.display().to_string(),
            "codex_dir": target_dirs.codex.display().to_string(),
            "agent_dirs": target_dirs
                .all
                .iter()
                .map(|dir| json!({
                    "agent": dir.agent,
                    "env_var": dir.env_var,
                    "path": dir.path.display().to_string()
                }))
                .collect::<Vec<_>>(),
            "remote_url": remote_url,
        }),
        warnings,
    )
}

fn panel_command_envelope(
    cmd: &str,
    result: std::result::Result<(serde_json::Value, crate::envelope::Meta), CommandFailure>,
) -> (StatusCode, Json<serde_json::Value>) {
    let request_id = uuid::Uuid::new_v4().to_string();
    match result {
        Ok((data, meta)) => (
            StatusCode::OK,
            Json(json!(Envelope::ok(cmd, request_id, data, meta))),
        ),
        Err(err) => (
            status_for_error_code(Some(err.code.as_str())),
            Json(json!(Envelope::err(
                cmd,
                request_id,
                err.code,
                err.message,
                err.details
            ))),
        ),
    }
}

fn panel_v1_ok(cmd: &str, data: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(json!(Envelope::ok(
            cmd,
            uuid::Uuid::new_v4().to_string(),
            data,
            crate::envelope::Meta::default()
        ))),
    )
}

fn panel_v1_registry_error(err: Json<serde_json::Value>) -> (StatusCode, Json<serde_json::Value>) {
    let status = status_for_registry_error_payload(&err.0);
    (status, err)
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
                ownership: req.ownership.unwrap_or(TargetOwnership::Observed),
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

pub(super) async fn registry_skill_save(
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

pub(super) async fn registry_skill_snapshot(
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

pub(super) async fn registry_skill_release(
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

pub(super) async fn registry_skill_rollback(
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

pub(super) async fn registry_orphan_clean(
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
                }),
            },
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
