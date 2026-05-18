use axum::{Json, http::StatusCode};
use serde_json::json;

use crate::commands::CommandFailure;
use crate::envelope::Envelope;
use crate::state_model::RegistryOperationRecord;

use super::super::auth::{status_for_error_code, status_for_registry_error_payload};

/// Accept `[a-z0-9_-]{1,64}` for `policy_profile`. The core CLI path enforces
/// the same shape; keeping the panel check here avoids doing a full command
/// dispatch for obviously malformed requests.
pub(super) fn policy_profile_looks_sane(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub(super) const DEFAULT_OPS_PAGE_SIZE: usize = 100;
pub(super) const MAX_OPS_PAGE_SIZE: usize = 250;

#[derive(Default)]
pub(super) struct OperationSummary {
    pub(super) request_id: Option<String>,
    pub(super) skill: Option<String>,
    pub(super) target: Option<String>,
    pub(super) binding: Option<String>,
    pub(super) method: Option<String>,
}

pub(super) fn operation_summary(op: &RegistryOperationRecord) -> OperationSummary {
    OperationSummary {
        request_id: json_string_field(&op.payload, &["request_id"]),
        skill: operation_skill_summary(op),
        target: json_string_field(&op.payload, &["target_id", "target"]),
        binding: json_string_field(&op.payload, &["binding_id", "binding"]),
        method: json_string_field(&op.payload, &["method"]),
    }
}

pub(super) fn operation_skill_summary(op: &RegistryOperationRecord) -> Option<String> {
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

pub(super) fn json_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

pub(super) fn panel_command_envelope(
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

pub(super) fn panel_v1_ok(cmd: &str, data: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
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

pub(super) fn panel_v1_registry_error(err: Json<serde_json::Value>) -> (StatusCode, Json<serde_json::Value>) {
    let status = status_for_registry_error_payload(&err.0);
    (status, err)
}
