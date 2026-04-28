use super::*;
use crate::panel::static_serve::{content_type_for, resolve_panel_asset_path};

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
