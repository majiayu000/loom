use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow};
use axum::{
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use include_dir::{Dir, include_dir};

use super::PanelState;

static PANEL_DIST: Dir<'_> = include_dir!("$OUT_DIR/panel-dist");

pub(super) async fn frontend_index(
    headers: HeaderMap,
    State(state): State<PanelState>,
) -> Response {
    let _ = state;
    serve_panel_asset("index.html", &headers)
}

pub(super) async fn frontend_static_asset(
    AxumPath(path): AxumPath<String>,
    headers: HeaderMap,
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
    serve_panel_asset(asset_path, &headers)
}

pub(super) fn ensure_panel_dist() -> Result<()> {
    if PANEL_DIST.get_file("index.html.gz").is_some() {
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

fn serve_panel_asset(path: impl AsRef<Path>, headers: &HeaderMap) -> Response {
    let path = path.as_ref();
    let compressed_path = gzip_path(path);
    match PANEL_DIST.get_file(&compressed_path) {
        Some(file) => {
            if !accepts_gzip(headers) {
                return Response::builder()
                    .status(StatusCode::NOT_ACCEPTABLE)
                    .header("vary", "Accept-Encoding")
                    .body(axum::body::Body::from(
                        "panel assets require gzip content encoding",
                    ))
                    .unwrap_or_else(|err| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("failed to build asset response: {}", err),
                        )
                            .into_response()
                    });
            }
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", content_type_for(path))
                .header("content-encoding", "gzip")
                .header("vary", "Accept-Encoding")
                .body(axum::body::Body::from(file.contents()))
                .unwrap_or_else(|err| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("failed to build asset response: {}", err),
                    )
                        .into_response()
                })
        }
        None => (
            StatusCode::NOT_FOUND,
            format!("panel asset not found: {}", path.display()),
        )
            .into_response(),
    }
}

fn gzip_path(path: &Path) -> PathBuf {
    let mut compressed = path.as_os_str().to_owned();
    compressed.push(".gz");
    PathBuf::from(compressed)
}

pub(crate) fn accepts_gzip(headers: &HeaderMap) -> bool {
    let values = headers.get_all("accept-encoding");
    if values.iter().next().is_none() {
        return true;
    }

    let mut explicit_gzip = None;
    let mut wildcard = None;
    for value in values {
        let Ok(value) = value.to_str() else {
            return false;
        };
        for item in value.split(',') {
            let mut parts = item.trim().split(';');
            let coding = parts.next().unwrap_or("").trim();
            let mut quality = 1.0_f32;
            for parameter in parts {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                if name.trim().eq_ignore_ascii_case("q") {
                    let Ok(parsed) = value.trim().parse::<f32>() else {
                        return false;
                    };
                    if !(0.0..=1.0).contains(&parsed) {
                        return false;
                    }
                    quality = parsed;
                }
            }
            if coding.eq_ignore_ascii_case("gzip") {
                explicit_gzip = Some(quality);
            } else if coding == "*" {
                wildcard = Some(quality);
            }
        }
    }
    explicit_gzip
        .or(wildcard)
        .is_some_and(|quality| quality > 0.0)
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

#[cfg(test)]
mod response_tests {
    use std::io::Read;

    use axum::body::to_bytes;
    use flate2::read::GzDecoder;

    use super::*;

    #[tokio::test]
    async fn embedded_index_response_is_valid_gzip_with_cache_headers() -> anyhow::Result<()> {
        let response = serve_panel_asset("index.html", &HeaderMap::new());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-encoding"], "gzip");
        assert_eq!(response.headers()["vary"], "Accept-Encoding");
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );

        let compressed = to_bytes(response.into_body(), 2 * 1024 * 1024).await?;
        let mut decoder = GzDecoder::new(compressed.as_ref());
        let mut decoded = String::new();
        decoder.read_to_string(&mut decoded)?;
        assert!(decoded.starts_with("<!doctype html>"));
        assert!(decoded.contains("<title>Loom Panel</title>"));
        Ok(())
    }

    #[tokio::test]
    async fn embedded_asset_rejects_identity_only_requests() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-encoding",
            axum::http::HeaderValue::from_static("identity"),
        );
        let response = serve_panel_asset("index.html", &headers);
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(response.headers()["vary"], "Accept-Encoding");

        let response = serve_panel_asset("missing.txt", &headers);
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
