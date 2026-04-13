use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use super::PanelState;

pub(super) async fn frontend_index(State(state): State<PanelState>) -> Response {
    serve_panel_asset(state.dist_dir.join("index.html"))
}

pub(super) async fn frontend_static_asset(
    AxumPath(path): AxumPath<String>,
    State(state): State<PanelState>,
) -> Response {
    let asset_path = match resolve_panel_asset_path(&state.dist_dir, &path) {
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

pub(super) fn ensure_panel_dist(dist_dir: &Path) -> Result<()> {
    let index_path = dist_dir.join("index.html");
    if index_path.is_file() {
        Ok(())
    } else {
        Err(anyhow!(
            "panel frontend not built; expected {}",
            index_path.display()
        ))
    }
}

pub(crate) fn resolve_panel_asset_path(dist_dir: &Path, requested: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in PathBuf::from(requested).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(dist_dir.join(relative))
}

fn serve_panel_asset(path: PathBuf) -> Response {
    match fs::read(&path) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", content_type_for(path.as_path()))
            .body(Body::from(bytes))
            .unwrap_or_else(|err| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to build asset response: {}", err),
                )
                    .into_response()
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (
            StatusCode::NOT_FOUND,
            format!("panel asset not found: {}", path.display()),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read panel asset {}: {}", path.display(), err),
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
