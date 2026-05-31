use axum::{Json, http::StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::commands::CommandFailure;
use crate::envelope::Envelope;

use super::super::auth::{status_for_error_code, status_for_registry_error_payload};

pub(super) const DEFAULT_OPS_PAGE_SIZE: usize = 100;
pub(super) const MAX_OPS_PAGE_SIZE: usize = 250;

#[derive(Debug, Default, Deserialize)]
pub(in crate::panel) struct ProjectionsQuery {
    #[serde(default)]
    pub(in crate::panel) health: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(in crate::panel) struct OpsQuery {
    #[serde(default)]
    pub(in crate::panel) limit: Option<usize>,
    #[serde(default)]
    pub(in crate::panel) offset: Option<usize>,
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

pub(super) fn panel_v1_ok(
    cmd: &str,
    data: serde_json::Value,
) -> (StatusCode, Json<serde_json::Value>) {
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

pub(super) fn panel_v1_registry_error(
    err: Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    let status = status_for_registry_error_payload(&err.0);
    (status, err)
}
