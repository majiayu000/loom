use super::*;
use crate::cli::{AgentKind, ProjectionMethod};
use crate::panel::handlers::{
    OpsQuery, TelemetryReportQuery, registry_orphan_clean, registry_skill_trash_add,
    registry_skill_trash_purge, registry_skill_trash_restore, registry_skill_use, remote_set,
    v1_health, v1_info, v1_overview, v1_pending, v1_registry_ops, v1_registry_targets,
    v1_skill_diagnose, v1_skill_inspect, v1_skill_trash, v1_skills, v1_telemetry_report,
    v1_workspace_status,
};
use crate::panel::{TrashRestoreRequest, UseRequest};
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryBindingRule, RegistryOperationRecord,
    RegistryProjectionInstance, RegistryProjectionsFile, RegistryRulesFile,
};
use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, Query},
    http::{HeaderMap, HeaderValue},
};
use chrono::Duration as ChrDuration;
use serde_json::{Value, json};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    process::Command,
};

fn git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn panel_peer() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000)
}

fn panel_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:43117"));
    headers
}

#[tokio::test]
async fn v1_telemetry_report_returns_cli_read_model_without_audit_write() {
    let (root, state) = make_test_state();
    let telemetry_dir = root.join("state/telemetry");
    fs::create_dir_all(&telemetry_dir).expect("create telemetry dir");
    fs::write(
        telemetry_dir.join("config.json"),
        r#"{
  "schema_version": 1,
  "enabled": true,
  "mode": "local-only",
  "redaction": "default",
  "retention_days": 90
}
"#,
    )
    .expect("write telemetry config");
    fs::write(
        telemetry_dir.join("events.jsonl"),
        r#"{"schema_version":1,"event_id":"evt_panel","event_type":"skill.eval","skill_id":"demo","agent":"codex","workspace_hash":"sha256:panel","timestamp":"2026-01-01T00:00:00Z","metrics":{"tokens_in":7,"commands":2,"success":true},"privacy":{"raw_prompt_stored":false,"raw_code_stored":false,"redacted":true}}
"#,
    )
    .expect("write telemetry events");

    let (status, Json(payload)) =
        v1_telemetry_report(Query(TelemetryReportQuery::default()), State(state)).await;

    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("telemetry.report"));
    assert_eq!(payload["data"]["matched_events"], json!(1));
    assert_eq!(payload["data"]["summary"]["value"]["eval_runs"], json!(1));
    assert_eq!(payload["data"]["summary"]["cost"]["tokens_in"], json!(7));
    assert_eq!(
        payload["data"]["panel_read_model"]["route"],
        json!("/api/v1/telemetry/report")
    );
    assert!(
        !root.join("state/events/commands.jsonl").exists(),
        "panel telemetry report must be read-only and avoid command audit writes"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn registry_skill_use_returns_plan_without_mutation() {
    let (root, state) = make_test_state();
    let skill_dir = root.join("skills/demo");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo skill for panel use planning.\n---\n# Demo\n",
    )
    .expect("write skill");

    let (status, Json(payload)) = registry_skill_use(
        AxumPath("demo".to_string()),
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
        Json(UseRequest {
            agents: vec![AgentKind::Claude],
            scope: None,
            workspace: Some(root.join("workspace")),
            profile: Some("panel".to_string()),
            method: Some(ProjectionMethod::Copy),
            target_root: None,
            adopt: false,
            apply: false,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("use"));
    assert_eq!(payload["data"]["dry_run"], json!(true));
    assert_eq!(payload["data"]["steps"][0]["agent"], json!("claude"));
    assert_eq!(payload["data"]["steps"][0]["method"], json!("copy"));
    assert!(
        !root.join("targets").exists(),
        "panel use planning must not create targets"
    );

    cleanup_root(root);
}

#[test]
fn v1_registry_ops_returns_bounded_newest_first_rows() {
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
            let (status, Json(payload)) = v1_registry_ops(
                Query(OpsQuery {
                    limit: Some(2),
                    offset: Some(0),
                }),
                State(state.clone()),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
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
async fn v1_registry_ops_returns_activity_summary_fields() {
    let (root, state) = make_test_state();
    let paths = RegistryStatePaths::from_app_context(state.ctx.as_ref());
    paths.ensure_layout().expect("ensure registry layout");
    let now = Utc::now();
    paths
        .append_operation(&RegistryOperationRecord {
            op_id: "op-activity".to_string(),
            intent: "skill.project".to_string(),
            status: "succeeded".to_string(),
            ack: false,
            payload: json!({
                "skill_id": "demo-skill",
                "binding_id": "binding-1",
                "target_id": "target-1",
                "method": "copy",
                "request_id": "req-1"
            }),
            effects: json!({}),
            last_error: None,
            created_at: now,
            updated_at: now,
        })
        .expect("append op");

    let (status, Json(payload)) = v1_registry_ops(
        Query(OpsQuery {
            limit: Some(10),
            offset: Some(0),
        }),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let op = &payload["data"]["operations"][0];
    assert_eq!(op["op_id"], json!("op-activity"));
    assert_eq!(op["skill"], json!("demo-skill"));
    assert_eq!(op["binding"], json!("binding-1"));
    assert_eq!(op["target"], json!("target-1"));
    assert_eq!(op["method"], json!("copy"));
    assert_eq!(op["request_id"], json!("req-1"));
    assert!(op.get("payload").is_none());
    assert!(op.get("effects").is_none());

    cleanup_root(root);
}

#[tokio::test]
async fn v1_registry_ops_includes_release_anchor_history_audit_without_registry_op_id() {
    let (root, state) = make_test_state();
    git_ok(&root, &["init"]);
    let paths = RegistryStatePaths::from_app_context(state.ctx.as_ref());
    paths.ensure_layout().expect("ensure registry layout");
    crate::gitops::append_history_audit_event(
        state.ctx.as_ref(),
        "skill.release",
        json!({
            "skill": "demo-skill",
            "tag": "snapshot/demo-skill/20260518T000000Z-deadbee"
        }),
        "req-anchor",
    )
    .expect("append history audit");

    let (status, Json(payload)) = v1_registry_ops(
        Query(OpsQuery {
            limit: Some(10),
            offset: Some(0),
        }),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["data"]["registry_count"], json!(0));
    assert_eq!(payload["data"]["audit_count"], json!(1));
    let op = &payload["data"]["operations"][0];
    assert_eq!(op["op_id"], Value::Null);
    assert!(op["audit_id"].as_str().is_some());
    assert_eq!(op["source"], json!("loom_history"));
    assert_eq!(op["intent"], json!("skill.release"));
    assert_eq!(op["status"], json!("succeeded"));
    assert_eq!(op["request_id"], json!("req-anchor"));
    assert_eq!(op["skill"], json!("demo-skill"));
    assert!(
        std::fs::read_to_string(paths.operations_file)
            .expect("read registry ops")
            .trim()
            .is_empty(),
        "snapshot audit must not be written as a registry operation"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn v1_health_returns_cli_envelope_shape() {
    let (status, Json(payload)) = v1_health().await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("panel.health"));
    assert_eq!(payload["error"], Value::Null);
    assert_eq!(payload["data"]["service"], json!("loom-panel"));
    assert_eq!(payload["meta"]["warnings"], json!([]));
}

#[tokio::test]
async fn v1_workspace_status_returns_cli_envelope_with_command_audit() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) = v1_workspace_status(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("workspace.status"));
    assert_eq!(payload["data"]["state_model"], json!("registry"));
    assert_eq!(payload["data"]["registry"]["available"], json!(false));
    let raw = fs::read_to_string(root.join("state/events/commands.jsonl"))
        .expect("read command event log");
    let events = raw
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse command event"))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["cmd"], json!("workspace.status"));
    assert_eq!(events[0]["status"], json!("started"));
    assert_eq!(events[1]["status"], json!("succeeded"));

    cleanup_root(root);
}

#[tokio::test]
async fn v1_overview_returns_workspace_status_payload() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) = v1_overview(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("panel.overview"));
    assert_eq!(payload["data"]["registered_targets"]["count"], json!(0));
    assert!(payload["data"]["remote"].is_object());

    cleanup_root(root);
}

#[tokio::test]
async fn v1_registry_targets_returns_non_2xx_when_registry_is_missing() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) = v1_registry_targets(State(state)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(status_code(&payload), Some("STATE_NOT_INITIALIZED"));

    cleanup_root(root);
}

#[tokio::test]
async fn v1_registry_targets_success_uses_cli_envelope_shape() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);

    let (status, Json(payload)) = v1_registry_targets(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("registry.targets"));
    assert_eq!(payload["error"], Value::Null);
    assert_eq!(payload["data"]["count"], json!(0));

    cleanup_root(root);
}

mod skill_endpoints;

#[tokio::test]
async fn v1_registry_ops_returns_non_2xx_when_registry_is_missing() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) = v1_registry_ops(
        Query(OpsQuery {
            limit: None,
            offset: None,
        }),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(status_code(&payload), Some("STATE_NOT_INITIALIZED"));

    cleanup_root(root);
}

#[tokio::test]
async fn registry_status_returns_bad_request_when_state_is_missing() {
    let (root, state) = make_test_state();

    let (status, payload) = run_registry_status(state).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(status_code(&payload), Some("STATE_NOT_INITIALIZED"));
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

    assert_eq!(
        crate::gitops::remote_url(&state.ctx)
            .expect("read remote")
            .as_deref(),
        Some(url)
    );

    cleanup_root(root);
}

#[tokio::test]
async fn registry_orphan_clean_uses_cli_envelope_and_records_operation() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let paths = RegistryStatePaths::from_root(&root);
    fs::write(
        &paths.projections_file,
        serde_json::to_vec_pretty(&RegistryProjectionsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projections: vec![RegistryProjectionInstance {
                instance_id: "inst-orphan".to_string(),
                skill_id: "skill.writer".to_string(),
                binding_id: None,
                target_id: "target-1".to_string(),
                materialized_path: root.join("live/skill.writer").display().to_string(),
                method: crate::core::vocab::ProjectionMethod::Copy,
                last_applied_rev: "deadbeef".to_string(),
                health: crate::core::vocab::Health::Orphaned,
                observed_drift: Some(false),
                updated_at: Some(Utc::now()),
            }],
        })
        .expect("serialize orphan projection"),
    )
    .expect("write orphan projection");

    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
    let mut headers = HeaderMap::new();
    headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:43117"));

    let (status, Json(payload)) = registry_orphan_clean(
        ConnectInfo(peer),
        headers,
        State(state),
        Json(super::super::OrphanCleanRequest {
            delete_live_paths: false,
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("skill.orphan.clean"));
    assert_eq!(payload["data"]["cleaned_count"], json!(1));
    assert!(payload["meta"]["op_id"].as_str().is_some());
    let snapshot = paths.load_snapshot().expect("load snapshot");
    assert!(snapshot.projections.projections.is_empty());

    cleanup_root(root);
}

#[tokio::test]
async fn v1_info_redacts_remote_credentials() {
    let (root, state) = make_test_state();
    let url =
        "https://user:pass@example.com/loom-registry.git?token=ghp_secret&ref=main#ghp_fragment";
    crate::gitops::ensure_repo_initialized(&state.ctx).expect("init repo");
    crate::gitops::set_remote_origin(&state.ctx, url).expect("set remote");

    let Json(info_payload) = v1_info(State(state)).await;
    let info_url = info_payload["data"]["remote_url"]
        .as_str()
        .expect("info remote url");
    assert!(!info_url.contains("user:pass"));
    assert!(!info_url.contains("ghp_secret"));
    assert!(info_url.contains("<redacted>"));
    assert_eq!(
        info_payload["meta"]["warnings"]
            .as_array()
            .expect("warnings array"),
        &Vec::<serde_json::Value>::new()
    );

    cleanup_root(root);
}

#[tokio::test]
async fn info_surfaces_warning_when_root_is_not_a_git_repository() {
    let (root, state) = make_test_state();
    // make_test_state creates the directory but never runs `git init`, so
    // `git remote get-url origin` exits 128 with "fatal: not a git
    // repository" — currently mapped to Ok(None) inside gitops::remote_url.
    // The handler should probe the repo and surface the misconfiguration.

    let Json(payload) = v1_info(State(state)).await;

    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["data"]["remote_url"], json!(""));
    let warnings = payload["meta"]["warnings"]
        .as_array()
        .expect("warnings array");
    assert_eq!(warnings.len(), 1);
    let message = warnings[0].as_str().expect("warning string");
    assert!(
        message.starts_with("git repository not initialized"),
        "unexpected warning: {message}"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn info_omits_warning_when_repo_initialized_but_no_remote_configured() {
    let (root, state) = make_test_state();
    crate::gitops::ensure_repo_initialized(&state.ctx).expect("init repo");

    let Json(payload) = v1_info(State(state)).await;

    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["data"]["remote_url"], json!(""));
    assert_eq!(
        payload["meta"]["warnings"]
            .as_array()
            .expect("warnings array"),
        &Vec::<serde_json::Value>::new()
    );

    cleanup_root(root);
}

#[tokio::test]
async fn info_surfaces_warning_when_git_remote_lookup_fails() {
    let (root, state) = make_test_state();
    // Remove the worktree out from under the panel to make `git remote get-url`
    // fail at spawn time (current_dir does not exist).
    fs::remove_dir_all(&root).expect("remove root");

    let Json(payload) = v1_info(State(state)).await;

    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["data"]["remote_url"], json!(""));
    let warnings = payload["meta"]["warnings"]
        .as_array()
        .expect("warnings array");
    assert_eq!(warnings.len(), 1);
    let message = warnings[0].as_str().expect("warning string");
    assert!(
        message.starts_with("failed to read git remote url"),
        "unexpected warning: {message}"
    );
}

#[tokio::test]
async fn pending_returns_non_2xx_with_structured_error_body_on_failure() {
    let (root, state) = make_test_state();
    let paths = crate::state_model::RegistryStatePaths::from_app_context(&state.ctx);
    paths.ensure_layout().expect("create registry ops layout");
    fs::remove_file(&paths.operations_file).expect("remove operations file");
    fs::create_dir_all(&paths.operations_file).expect("replace operations file with dir");

    let (status, Json(payload)) = v1_pending(State(state)).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["error"]["code"], json!("IO_ERROR"));
    assert!(payload["error"]["message"].as_str().is_some());

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn pending_returns_ok_with_empty_report_on_success() {
    let (root, state) = make_test_state();
    let paths = crate::state_model::RegistryStatePaths::from_app_context(&state.ctx);
    paths.ensure_layout().expect("create registry ops layout");

    let (status, Json(payload)) = v1_pending(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("operation_backlog.list"));
    assert!(payload["request_id"].as_str().is_some());
    assert_eq!(payload["data"]["count"], json!(0));
    assert!(payload["data"]["ops"].as_array().is_some());

    let _ = fs::remove_dir_all(root);
}
