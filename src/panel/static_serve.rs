use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use include_dir::{Dir, include_dir};

use super::PanelState;

static PANEL_DIST: Dir<'_> = include_dir!("$OUT_DIR/panel-dist");

pub(super) async fn frontend_index(State(state): State<PanelState>) -> Response {
    let _ = state;
    serve_panel_asset("index.html")
}

pub(super) async fn frontend_static_asset(
    AxumPath(path): AxumPath<String>,
    State(state): State<PanelState>,
) -> Response {
    let _ = state;
    let asset_path = match resolve_panel_asset_path(&path) {
        Some(path) => path,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "invalid panel asset path".to_string(),
            )
                .into_response();
        }
    };
    serve_panel_asset(asset_path)
}

pub(super) fn ensure_panel_dist() -> Result<()> {
    if PANEL_DIST.get_file("index.html").is_some() {
        Ok(())
    } else {
        Err(anyhow!(
            "panel frontend assets are unavailable in this build; rebuild with Bun installed or run from a checkout that has panel/dist"
        ))
    }
}

pub(crate) fn resolve_panel_asset_path(requested: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in PathBuf::from(requested).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(relative)
}

fn serve_panel_asset(path: impl AsRef<Path>) -> Response {
    let path = path.as_ref();
    match PANEL_DIST.get_file(path) {
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", content_type_for(path))
            .body(axum::body::Body::from(file.contents()))
            .unwrap_or_else(|err| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to build asset response: {}", err),
                )
                    .into_response()
            }),
        None => (
            StatusCode::NOT_FOUND,
            format!("panel asset not found: {}", path.display()),
        )
            .into_response(),
    }
}

pub(crate) fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}
