use super::*;
use crate::panel::handlers::{OpsQuery, info, pending, registry_ops, remote_set, remote_status};
use crate::state_model::{REGISTRY_SCHEMA_VERSION, RegistryOperationRecord};
use axum::{
    Json,
    extract::{ConnectInfo, Query},
    http::{HeaderMap, HeaderValue},
};
use chrono::Duration as ChrDuration;
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[test]
fn registry_ops_returns_bounded_newest_first_rows() {
    let (root, state) = make_test_state();
    let paths = RegistryStatePaths::from_app_context(state.ctx.as_ref());
    paths.ensure_layout().expect("ensure registry layout");

    let now = Utc::now();
    for index in 0..3 {
        paths
            .append_operation(&RegistryOperationRecord {
                op_id: format!("op-{index}"),
                intent: "skill.project".to_string(),
                status: "succeeded".to_string(),
                ack: index % 2 == 0,
                payload: json!({ "blob": "ignored" }),
                effects: json!({ "index": index }),
                last_error: None,
                created_at: now + ChrDuration::seconds(index as i64),
                updated_at: now + ChrDuration::seconds(index as i64),
            })
            .expect("append op");
    }

    let payload = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let Json(payload) = registry_ops(
                Query(OpsQuery {
                    limit: Some(2),
                    offset: Some(0),
                }),
                State(state.clone()),
            )
            .await;
            payload
        });

    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["data"]["count"], json!(3));
    assert_eq!(payload["data"]["loaded_count"], json!(2));
    assert_eq!(payload["data"]["has_more"], json!(true));
    let operations = payload["data"]["operations"].as_array().expect("ops array");
    assert_eq!(operations[0]["op_id"], json!("op-2"));
    assert_eq!(operations[1]["op_id"], json!("op-1"));
    assert!(operations[0].get("payload").is_none());
    assert!(operations[0].get("effects").is_none());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn registry_status_returns_bad_request_when_state_is_missing() {
    let (root, state) = make_test_state();

    let (status, payload) = run_registry_status(state).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(status_code(&payload), Some("ARG_INVALID"));
    assert!(
        payload["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("registry state not initialized"))
    );

    cleanup_root(root);
}

#[tokio::test]
async fn registry_status_returns_internal_error_when_state_is_corrupt() {
    let (root, state) = make_test_state();
    let paths = RegistryStatePaths::from_root(&root);
    fs::create_dir_all(&paths.registry_dir).expect("create registry dir");
    fs::write(&paths.schema_file, b"{not-json").expect("write corrupt schema");

    let (status, payload) = run_registry_status(state).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(status_code(&payload), Some("STATE_CORRUPT"));
    assert!(payload["error"]["message"].as_str().is_some());

    cleanup_root(root);
}

#[tokio::test]
async fn registry_status_returns_internal_error_when_schema_mismatches() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION + 1);

    let (status, payload) = run_registry_status(state).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(status_code(&payload), Some("SCHEMA_MISMATCH"));
    assert!(
        payload["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("schema version mismatch"))
    );

    cleanup_root(root);
}

#[tokio::test]
async fn registry_status_returns_ok_when_snapshot_loads() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);

    let (status, payload) = run_registry_status(state).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(
        payload["data"]["schema_version"],
        json!(REGISTRY_SCHEMA_VERSION)
    );
    assert_eq!(payload["data"]["counts"]["targets"], json!(0));
    assert_eq!(payload["data"]["counts"]["bindings"], json!(0));

    cleanup_root(root);
}

#[tokio::test]
async fn remote_status_returns_non_2xx_with_structured_error_body_on_failure() {
    let (root, state) = make_test_state();
    state
        .ctx
        .ensure_state_layout()
        .expect("create pending ops layout");
    fs::remove_file(&state.ctx.pending_ops_file).expect("remove pending ops file");
    fs::create_dir_all(&state.ctx.pending_ops_file).expect("replace pending ops file with dir");

    let (status, Json(payload)) = remote_status(State(state)).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["error"]["code"], json!("IO_ERROR"));
    assert!(payload["error"]["message"].as_str().is_some());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn remote_status_returns_success_payload_when_remote_is_not_configured() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) = remote_status(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("remote.status"));
    assert!(payload["request_id"].as_str().is_some());
    assert_eq!(payload["data"]["remote"]["configured"], json!(false));
    assert!(payload["data"]["remote"].is_object());
    assert!(payload["data"]["warnings"].is_array());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn remote_set_rejects_empty_url() {
    let (root, state) = make_test_state();
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
    let mut headers = HeaderMap::new();
    headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:43117"));

    let (status, Json(payload)) = remote_set(
        ConnectInfo(peer),
        headers,
        State(state),
        Json(super::super::RemoteSetRequest {
            url: "   ".to_string(),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["cmd"], json!("workspace.remote.set"));
    assert_eq!(payload["error"]["code"], json!("ARG_INVALID"));

    cleanup_root(root);
}

#[tokio::test]
async fn remote_set_configures_origin_from_authorized_panel_request() {
    let (root, state) = make_test_state();
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
    let mut headers = HeaderMap::new();
    headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:43117"));
    let url = "https://example.com/loom-registry.git";

    let (status, Json(payload)) = remote_set(
        ConnectInfo(peer),
        headers,
        State(state.clone()),
        Json(super::super::RemoteSetRequest {
            url: format!("  {url}  "),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("workspace.remote"));
    assert_eq!(payload["data"]["remote"], json!("origin"));
    assert_eq!(payload["data"]["url"], json!(url));

    let (remote_status_code, Json(remote_payload)) = remote_status(State(state)).await;
    assert_eq!(remote_status_code, StatusCode::OK);
    assert_eq!(remote_payload["data"]["remote"]["configured"], json!(true));
    assert_eq!(remote_payload["data"]["remote"]["url"], json!(url));

    cleanup_root(root);
}

#[tokio::test]
async fn info_and_remote_status_redact_remote_credentials() {
    let (root, state) = make_test_state();
    let url =
        "https://user:pass@example.com/loom-registry.git?token=ghp_secret&ref=main#ghp_fragment";
    crate::gitops::ensure_repo_initialized(&state.ctx).expect("init repo");
    crate::gitops::set_remote_origin(&state.ctx, url).expect("set remote");

    let Json(info_payload) = info(State(state.clone())).await;
    let info_url = info_payload["data"]["remote_url"]
        .as_str()
        .expect("info remote url");
    assert!(!info_url.contains("user:pass"));
    assert!(!info_url.contains("ghp_secret"));
    assert!(info_url.contains("<redacted>"));

    let (status, Json(remote_payload)) = remote_status(State(state)).await;
    assert_eq!(status, StatusCode::OK);
    let status_url = remote_payload["data"]["remote"]["url"]
        .as_str()
        .expect("status remote url");
    assert!(!status_url.contains("user:pass"));
    assert!(!status_url.contains("ghp_secret"));
    assert!(status_url.contains("<redacted>"));

    cleanup_root(root);
}

#[tokio::test]
async fn pending_returns_non_2xx_with_structured_error_body_on_failure() {
    let (root, state) = make_test_state();
    state
        .ctx
        .ensure_state_layout()
        .expect("create pending ops layout");
    fs::remove_file(&state.ctx.pending_ops_file).expect("remove pending ops file");
    fs::create_dir_all(&state.ctx.pending_ops_file).expect("replace pending ops file with dir");

    let (status, Json(payload)) = pending(State(state)).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["error"]["code"], json!("IO_ERROR"));
    assert!(payload["error"]["message"].as_str().is_some());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn pending_returns_ok_with_empty_report_on_success() {
    let (root, state) = make_test_state();
    state
        .ctx
        .ensure_state_layout()
        .expect("create pending ops layout");

    let (status, Json(payload)) = pending(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("pending.list"));
    assert!(payload["request_id"].as_str().is_some());
    assert_eq!(payload["data"]["count"], json!(0));
    assert!(payload["data"]["ops"].as_array().is_some());

    let _ = fs::remove_dir_all(root);
}
