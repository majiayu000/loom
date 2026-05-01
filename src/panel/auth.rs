use std::net::SocketAddr;

use axum::{
    Json,
    http::{HeaderMap, StatusCode, Uri},
};
use serde_json::json;
use uuid::Uuid;

use crate::cli::{Cli, Command};
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
    let origin_matches = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == panel_origin);
    if origin_matches {
        return true;
    }

    headers
        .get("referer")
        .and_then(|value| value.to_str().ok())
        .and_then(referer_origin)
        .is_some_and(|value| value == panel_origin)
}

fn referer_origin(referer: &str) -> Option<String> {
    let uri: Uri = referer.parse().ok()?;
    Some(format!(
        "{}://{}",
        uri.scheme_str()?,
        uri.authority()?.as_str()
    ))
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
        "ARG_INVALID" => StatusCode::BAD_REQUEST,
        "SKILL_NOT_FOUND" | "BINDING_NOT_FOUND" | "TARGET_NOT_FOUND" => StatusCode::NOT_FOUND,
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
        Some("ARG_INVALID") => StatusCode::BAD_REQUEST,
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
    json!({
        "ok": false,
        "cmd": cmd,
        "request_id": request_id,
        "version": env!("CARGO_PKG_VERSION"),
        "data": {},
        "error": {
            "code": code,
            "message": message,
            "details": {}
        },
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
            "ARG_INVALID",
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
    Json(json!({
        "ok": true,
        "cmd": cmd,
        "request_id": Uuid::new_v4().to_string(),
        "version": env!("CARGO_PKG_VERSION"),
        "data": data,
        "meta": {
            "warnings": []
        }
    }))
}

pub(super) fn registry_error(cmd: &str, code: &str, message: String) -> Json<serde_json::Value> {
    let request_id = Uuid::new_v4().to_string();
    Json(error_envelope(cmd, &request_id, code, &message))
}
