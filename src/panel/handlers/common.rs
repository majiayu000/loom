use axum::{Json, http::StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::envelope::Envelope;

use super::super::auth::status_for_registry_error_payload;

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
