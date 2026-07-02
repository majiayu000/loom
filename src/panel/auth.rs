use std::net::SocketAddr;

use axum::{
    Json,
    extract::{Request, State},
    http::{
        HeaderMap, StatusCode, Uri,
        header::{HOST, ORIGIN, REFERER},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use uuid::Uuid;

use crate::cli::{Cli, Command};
use crate::error_actions::default_next_actions;
use crate::state::AppContext;
use crate::state_model::RegistryStatePaths;

use super::PanelState;

pub(crate) fn ensure_mutation_authorized(
    state: &PanelState,
    peer: SocketAddr,
    headers: &HeaderMap,
    cmd: &str,
) -> Option<(StatusCode, Json<serde_json::Value>)> {
    if peer.ip().is_loopback() && request_origin_matches(&state.panel_origin, headers) {
        return None;
    }
    let request_id = Uuid::new_v4().to_string();
    Some((
        StatusCode::FORBIDDEN,
        Json(error_envelope(
            cmd,
            &request_id,
            "UNAUTHORIZED",
            "unauthorized panel mutation request",
        )),
    ))
}

pub(crate) fn request_origin_matches(panel_origin: &str, headers: &HeaderMap) -> bool {
    let mut found_source_header = false;

    if let Some(origin) = headers.get(ORIGIN) {
        found_source_header = true;
        if !origin
            .to_str()
            .ok()
            .is_some_and(|value| panel_origin_matches(panel_origin, value))
        {
            return false;
        }
    }

    if let Some(referer) = headers.get(REFERER) {
        found_source_header = true;
        if !referer
            .to_str()
            .ok()
            .and_then(referer_origin)
            .is_some_and(|value| panel_origin_matches(panel_origin, &value))
        {
            return false;
        }
    }

    found_source_header
}

pub(crate) async fn ensure_panel_request_authorized(
    State(state): State<PanelState>,
    request: Request,
    next: Next,
) -> Response {
    if panel_request_authorized(&state.panel_origin, request.headers()) {
        return next.run(request).await;
    }

    StatusCode::FORBIDDEN.into_response()
}

pub(crate) fn panel_request_authorized(panel_origin: &str, headers: &HeaderMap) -> bool {
    panel_host_matches(panel_origin, headers)
        && request_source_origin_allowed(panel_origin, headers)
}

pub(crate) fn panel_host_matches(panel_origin: &str, headers: &HeaderMap) -> bool {
    headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| panel_authority_matches(panel_origin, value))
}

fn request_source_origin_allowed(panel_origin: &str, headers: &HeaderMap) -> bool {
    let has_origin = headers.contains_key(ORIGIN);
    let has_referer = headers.contains_key(REFERER);

    if !has_origin && !has_referer {
        return true;
    }

    request_origin_matches(panel_origin, headers)
}

fn referer_origin(referer: &str) -> Option<String> {
    let uri: Uri = referer.parse().ok()?;
    Some(format!(
        "{}://{}",
        uri.scheme_str()?,
        uri.authority()?.as_str()
    ))
}

fn panel_origin_matches(panel_origin: &str, origin: &str) -> bool {
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return false;
    }

    panel_authority_matches(panel_origin, authority)
}

fn panel_authority_matches(panel_origin: &str, authority: &str) -> bool {
    let Some(expected_port) = panel_origin_port(panel_origin) else {
        return false;
    };
    let Some((host, port)) = authority.rsplit_once(':') else {
        return false;
    };

    port == expected_port.as_str()
        && (host == "127.0.0.1" || host.eq_ignore_ascii_case("localhost"))
}

fn panel_origin_port(panel_origin: &str) -> Option<String> {
    let uri: Uri = panel_origin.parse().ok()?;
    let (_, port) = uri.authority()?.as_str().rsplit_once(':')?;
    Some(port.to_string())
}

pub(crate) fn run_panel_command(
    state: &PanelState,
    cmd: &str,
    success_status: StatusCode,
    command: Command,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = crate::commands::App {
        ctx: (*state.ctx).clone(),
    };
    let request_id = Uuid::new_v4().to_string();
    let cli = Cli {
        json: true,
        pretty: false,
        request_id: Some(request_id.clone()),
        root: Some(state.ctx.root.clone()),
        command,
    };

    match app.execute(cli) {
        Ok((envelope, _code)) => {
            let status = if envelope.ok {
                success_status
            } else {
                status_for_error_code(envelope.error.as_ref().map(|error| error.code.as_str()))
            };
            let payload = serde_json::to_value(envelope).unwrap_or_else(|err| {
                error_envelope(cmd, &request_id, "INTERNAL_ERROR", &err.to_string())
            });
            (status, Json(payload))
        }
        Err(err) => {
            let msg = err.to_string();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(error_envelope(cmd, &request_id, "INTERNAL_ERROR", &msg)),
            )
        }
    }
}

pub(crate) fn status_for_error_code(code: Option<&str>) -> StatusCode {
    match code.unwrap_or("INTERNAL_ERROR") {
        "ARG_INVALID" | "STATE_NOT_INITIALIZED" => StatusCode::BAD_REQUEST,
        "SKILL_NOT_FOUND" | "BINDING_NOT_FOUND" | "TARGET_NOT_FOUND" | "TRASH_ENTRY_NOT_FOUND" => {
            StatusCode::NOT_FOUND
        }
        "TARGET_NOT_MANAGED"
        | "TARGET_AGENT_MISMATCH"
        | "PROJECTION_CONFLICT"
        | "PROJECTION_METHOD_UNSUPPORTED"
        | "POLICY_BLOCKED"
        | "CAPTURE_CONFLICT" => StatusCode::CONFLICT,
        "LOCK_BUSY"
        | "DEPENDENCY_CONFLICT"
        | "REMOTE_DIVERGED"
        | "PUSH_REJECTED"
        | "REPLAY_CONFLICT" => StatusCode::CONFLICT,
        "UNAUTHORIZED" => StatusCode::FORBIDDEN,
        "REMOTE_UNREACHABLE" => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn status_for_registry_state_load_error(code: Option<&str>) -> StatusCode {
    match code {
        Some("ARG_INVALID" | "STATE_NOT_INITIALIZED") => StatusCode::BAD_REQUEST,
        Some("SCHEMA_MISMATCH" | "STATE_CORRUPT") => StatusCode::INTERNAL_SERVER_ERROR,
        _ => status_for_error_code(code),
    }
}

pub(crate) fn status_for_registry_error_payload(payload: &serde_json::Value) -> StatusCode {
    let code = payload
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(serde_json::Value::as_str);
    status_for_registry_state_load_error(code)
}

pub(crate) fn error_envelope(
    cmd: &str,
    request_id: &str,
    code: &str,
    message: &str,
) -> serde_json::Value {
    let next_actions = default_next_actions(code);
    let mut error = json!({
        "code": code,
        "message": message,
        "details": {}
    });
    if !next_actions.is_empty() {
        error["next_actions"] = json!(next_actions);
    }
    json!({
        "ok": false,
        "cmd": cmd,
        "request_id": request_id,
        "version": env!("CARGO_PKG_VERSION"),
        "data": {},
        "error": error,
        "meta": {
            "warnings": []
        }
    })
}

pub(super) fn load_registry_snapshot(
    ctx: &AppContext,
    cmd: &str,
) -> std::result::Result<crate::state_model::RegistrySnapshot, Json<serde_json::Value>> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    match paths.maybe_load_snapshot() {
        Ok(Some(snapshot)) => Ok(snapshot),
        Ok(None) => Err(registry_error(
            cmd,
            "STATE_NOT_INITIALIZED",
            format!(
                "registry state not initialized under {}",
                paths.registry_dir.display()
            ),
        )),
        Err(err) => {
            let message = err.to_string();
            let code = if message.contains("schema version mismatch") {
                "SCHEMA_MISMATCH"
            } else {
                "STATE_CORRUPT"
            };
            Err(registry_error(cmd, code, message))
        }
    }
}

pub(super) fn registry_ok(cmd: &str, data: serde_json::Value) -> Json<serde_json::Value> {
    registry_ok_with_warnings(cmd, data, Vec::new())
}

pub(super) fn registry_ok_with_warnings(
    cmd: &str,
    data: serde_json::Value,
    warnings: Vec<String>,
) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "cmd": cmd,
        "request_id": Uuid::new_v4().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        "data": data,
        "meta": {
            "warnings": warnings,
        }
    }))
}

pub(super) fn registry_error(cmd: &str, code: &str, message: String) -> Json<serde_json::Value> {
    let request_id = Uuid::new_v4().to_string();
    Json(error_envelope(cmd, &request_id, code, &message))
}
