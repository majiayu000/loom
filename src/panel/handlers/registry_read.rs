use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
};
use serde_json::json;

use super::super::auth::{
    load_registry_snapshot, registry_error, registry_ok, status_for_registry_error_payload,
};
use super::super::PanelState;
use super::shared::{DEFAULT_OPS_PAGE_SIZE, MAX_OPS_PAGE_SIZE};
use super::{OpsQuery, ProjectionsQuery};

pub(crate) async fn registry_status(
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
pub(crate) async fn registry_ops(
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

pub(crate) async fn registry_projections(
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

pub(crate) async fn registry_bindings(
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
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

pub(crate) async fn registry_binding_show(
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

pub(crate) async fn registry_targets(
    State(state): State<PanelState>,
) -> Json<serde_json::Value> {
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

pub(crate) async fn registry_target_show(
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
