use super::*;
use crate::panel::handlers::registry_skill_import_observed;
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryProjectionTarget, RegistryTargetCapabilities,
    RegistryTargetsFile,
};
use axum::{
    Json,
    extract::ConnectInfo,
    http::{HeaderMap, HeaderValue},
};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[tokio::test]
async fn registry_skill_import_observed_imports_existing_observed_skill() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let observed = root.join("observed-skills");
    fs::create_dir_all(observed.join("alpha")).expect("create observed skill");
    fs::write(observed.join("alpha/SKILL.md"), "# alpha\n").expect("write observed skill");

    let paths = RegistryStatePaths::from_root(&root);
    paths
        .save_targets(&RegistryTargetsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            targets: vec![RegistryProjectionTarget {
                target_id: "target-observed".to_string(),
                agent: "claude".into(),
                path: observed.display().to_string(),
                ownership: crate::core::vocab::Ownership::Observed,
                capabilities: RegistryTargetCapabilities {
                    symlink: true,
                    copy: true,
                    watch: true,
                },
                created_at: Some(Utc::now()),
            }],
        })
        .expect("save observed target");

    let mut headers = HeaderMap::new();
    headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:43117"));
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49152);

    let (status, Json(payload)) = registry_skill_import_observed(
        ConnectInfo(peer),
        headers,
        State(state),
        Json(ImportObservedRequest { target: None }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("skill.import_observed"));
    assert_eq!(payload["data"]["count"], json!(1));
    assert_eq!(payload["data"]["imported"][0]["skill"], json!("alpha"));
    assert!(root.join("skills/alpha/SKILL.md").exists());

    cleanup_root(root);
}
