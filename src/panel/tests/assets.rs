use super::*;
use crate::panel::static_serve::{accepts_gzip, content_type_for, resolve_panel_asset_path};
use axum::http::{HeaderMap, HeaderValue};

#[test]
fn resolve_panel_asset_path_rejects_invalid_components() {
    assert_eq!(
        resolve_panel_asset_path("assets/index.js"),
        Some(Path::new("assets/index.js").to_path_buf())
    );
    assert_eq!(
        resolve_panel_asset_path("./assets/index.css"),
        Some(Path::new("assets/index.css").to_path_buf())
    );
    assert_eq!(resolve_panel_asset_path("../secret.txt"), None);
    assert_eq!(resolve_panel_asset_path("/etc/passwd"), None);
}

#[test]
fn content_type_for_maps_known_panel_extensions() {
    assert_eq!(
        content_type_for(Path::new("index.html")),
        "text/html; charset=utf-8"
    );
    assert_eq!(
        content_type_for(Path::new("bundle.js")),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        content_type_for(Path::new("styles.css")),
        "text/css; charset=utf-8"
    );
    assert_eq!(content_type_for(Path::new("favicon.svg")), "image/svg+xml");
    assert_eq!(content_type_for(Path::new("font.woff2")), "font/woff2");
    assert_eq!(
        content_type_for(Path::new("artifact.bin")),
        "application/octet-stream"
    );
}

#[test]
fn gzip_negotiation_accepts_absent_explicit_and_wildcard_encodings() {
    let headers = HeaderMap::new();
    assert!(accepts_gzip(&headers));

    let mut headers = HeaderMap::new();
    headers.insert(
        "accept-encoding",
        HeaderValue::from_static("br, gzip;q=0.5"),
    );
    assert!(accepts_gzip(&headers));

    headers.insert("accept-encoding", HeaderValue::from_static("br, *;q=0.2"));
    assert!(accepts_gzip(&headers));
}

#[test]
fn gzip_negotiation_rejects_explicit_opt_out_and_malformed_quality() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "accept-encoding",
        HeaderValue::from_static("gzip;q=0, *;q=1"),
    );
    assert!(!accepts_gzip(&headers));

    headers.insert("accept-encoding", HeaderValue::from_static("identity"));
    assert!(!accepts_gzip(&headers));

    headers.insert("accept-encoding", HeaderValue::from_static("gzip;q=bogus"));
    assert!(!accepts_gzip(&headers));
}
