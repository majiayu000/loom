use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Result, anyhow};
use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use walkdir::WalkDir;

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
    if !index_path.is_file() {
        return Err(anyhow!(
            "panel frontend not built; expected {}",
            index_path.display()
        ));
    }

    let panel_dir = dist_dir.parent().ok_or_else(|| {
        anyhow!(
            "panel frontend not built; invalid dist directory {}",
            dist_dir.display()
        )
    })?;
    let (built_asset, built_at) = oldest_panel_dist_artifact(dist_dir)?;

    if let Some((source_path, source_mtime)) = newest_panel_build_input(panel_dir)?
        && source_mtime > built_at
    {
        return Err(anyhow!(
            "panel frontend assets are stale: {} is newer than {}. Run `make panel-build` or `cd panel && bun run build`.",
            source_path.display(),
            built_asset.display()
        ));
    }

    Ok(())
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

fn newest_panel_build_input(panel_dir: &Path) -> Result<Option<(PathBuf, SystemTime)>> {
    let mut newest = None;

    for relative in [
        "index.html",
        "landing.html",
        "package.json",
        "package-lock.json",
        "bun.lock",
        "tsconfig.json",
        "vite.config.ts",
    ] {
        update_newest_file(panel_dir.join(relative), &mut newest)?;
    }

    for relative in ["src", "public"] {
        let path = panel_dir.join(relative);
        if path.is_dir() {
            update_newest_from_tree(&path, &mut newest)?;
        }
    }

    Ok(newest)
}

fn oldest_panel_dist_artifact(dist_dir: &Path) -> Result<(PathBuf, SystemTime)> {
    let mut oldest = None;

    for relative in ["index.html", "landing.html", "favicon.svg"] {
        update_oldest_file(dist_dir.join(relative), &mut oldest)?;
    }

    for relative in ["index.html", "landing.html"] {
        let html_path = dist_dir.join(relative);
        if !html_path.is_file() {
            continue;
        }

        for asset_path in tracked_panel_dist_assets(&html_path)? {
            update_oldest_file(dist_dir.join(asset_path), &mut oldest)?;
        }
    }

    oldest.ok_or_else(|| {
        anyhow!(
            "panel frontend not built; expected assets under {}",
            dist_dir.display()
        )
    })
}

fn tracked_panel_dist_assets(html_path: &Path) -> Result<Vec<PathBuf>> {
    let html = fs::read_to_string(html_path)?;
    let mut tracked = Vec::new();
    let mut search_from = 0;

    while let Some(offset) = html[search_from..].find("assets/") {
        let start = search_from + offset;
        let start = if start > 0 && html.as_bytes()[start - 1] == b'/' {
            start - 1
        } else {
            start
        };
        let end = html[start..]
            .find(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '?' | '#' | ' ' | ')' | '>' | '<' | '\n' | '\r' | '\t'
                )
            })
            .map(|delta| start + delta)
            .unwrap_or(html.len());

        if let Some(asset_path) = normalize_panel_dist_asset_reference(&html[start..end])
            && !tracked.iter().any(|existing| existing == &asset_path)
        {
            tracked.push(asset_path);
        }

        search_from = end;
    }

    Ok(tracked)
}

fn normalize_panel_dist_asset_reference(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.split(['?', '#']).next()?.trim_start_matches('/');
    if !trimmed.starts_with("assets/") {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            _ => return None,
        }
    }
    Some(normalized)
}

fn update_newest_from_tree(root: &Path, newest: &mut Option<(PathBuf, SystemTime)>) -> Result<()> {
    for entry in WalkDir::new(root) {
        let entry = entry?;
        if entry.file_type().is_file() {
            update_newest_file(entry.path().to_path_buf(), newest)?;
        }
    }
    Ok(())
}

fn update_newest_file(path: PathBuf, newest: &mut Option<(PathBuf, SystemTime)>) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }

    let modified = fs::metadata(&path)?.modified()?;
    match newest {
        Some((_, newest_time)) if modified <= *newest_time => {}
        _ => *newest = Some((path, modified)),
    }
    Ok(())
}

fn update_oldest_file(path: PathBuf, oldest: &mut Option<(PathBuf, SystemTime)>) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }

    let modified = fs::metadata(&path)?.modified()?;
    match oldest {
        Some((_, oldest_time)) if modified >= *oldest_time => {}
        _ => *oldest = Some((path, modified)),
    }
    Ok(())
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
