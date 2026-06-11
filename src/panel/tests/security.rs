use super::*;
use crate::cli::{
    BindingAddArgs, CaptureArgs, Command, OpsCommand, ProjectArgs, ProjectionMethod, ReleaseArgs,
    RemoteCommand, RollbackArgs, SaveArgs, SkillCommand, SkillOnlyArgs, SkillTrashCommand,
    SyncCommand, TargetAddArgs, TargetCommand, TargetOwnership, TrashPurgeArgs, TrashRestoreArgs,
    WorkspaceBindingCommand, WorkspaceCommand, WorkspaceInitArgs, WorkspaceMatcherKind,
};
use crate::panel::auth::{
    ensure_mutation_authorized, error_envelope, panel_host_matches, panel_request_authorized,
    request_origin_matches, run_panel_command, status_for_error_code,
    status_for_registry_error_payload, status_for_registry_state_load_error,
};
use crate::state_model::REGISTRY_SCHEMA_VERSION;
use axum::http::{
    HeaderMap, HeaderValue,
    header::{HOST, ORIGIN, REFERER},
};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// Exhaustive list of every panel mutation command. Must stay in sync with the
// 22-row table in docs/LOOM_ARCHITECTURE_DECISIONS.md section 4.1 and the
// route registrations in `run_panel`.
const MUTATION_COMMANDS: &[&str] = &[
    "workspace.init",
    "target.add",
    "target.remove",
    "workspace.binding.add",
    "workspace.binding.remove",
    "skill.project",
    "skill.capture",
    "skill.save",
    "skill.snapshot",
    "skill.release",
    "skill.rollback",
    "skill.trash.add",
    "skill.trash.restore",
    "skill.trash.purge",
    "skill.orphan.clean",
    "workspace.remote.set",
    "ops.retry",
    "ops.purge",
    "ops.history.repair",
    "sync.push",
    "sync.pull",
    "sync.replay",
];

const V1_REGISTRY_READ_ROUTES: &[&str] = &[
    "/api/v1/health",
    "/api/v1/workspace/info",
    "/api/v1/registry/status",
    "/api/v1/ops/pending",
    "/api/v1/ops/diagnose",
    "/api/v1/bindings/{binding_id}",
    "/api/v1/targets/{target_id}",
    "/api/v1/skills/{skill_name}/history",
    "/api/v1/skills/{skill_name}/diff",
];

const LEGACY_PANEL_ROUTES: &[&str] = &[
    "/api/health",
    "/api/info",
    "/api/skills",
    "/api/pending",
    "/api/remote/status",
    "/api/remote/set",
    "/api/ops/retry",
    "/api/ops/purge",
    "/api/ops/history/repair",
    "/api/sync/push",
    "/api/sync/pull",
    "/api/sync/replay",
    "/api/registry/status",
    "/api/registry/ops",
    "/api/registry/ops/diagnose",
    "/api/registry/projections",
    "/api/registry/bindings",
    "/api/registry/bindings/{binding_id}",
    "/api/registry/bindings/{binding_id}/remove",
    "/api/registry/targets",
    "/api/registry/targets/{target_id}",
    "/api/registry/targets/{target_id}/remove",
    "/api/registry/skills",
    "/api/registry/skills/{skill_name}/history",
    "/api/registry/skills/{skill_name}/diff",
    "/api/registry/project",
    "/api/registry/capture",
    "/api/registry/orphans/clean",
];

#[test]
fn mutation_commands_count_is_twenty_two() {
    assert_eq!(MUTATION_COMMANDS.len(), 22);
}

#[test]
fn panel_routes_are_v1_only_without_legacy_api_compatibility() {
    let client_source = include_str!("../../../panel/src/lib/api/client.ts");
    let panel_routes = include_str!("../mod.rs");

    for route in V1_REGISTRY_READ_ROUTES {
        assert!(
            panel_routes.contains(route),
            "missing v1 registry read route {route}"
        );
    }

    for route in LEGACY_PANEL_ROUTES {
        assert!(
            !panel_routes.contains(route),
            "legacy panel API route must be removed: {route}"
        );
    }

    assert!(
        !client_source.contains("\"/api/health")
            && !client_source.contains("\"/api/info")
            && !client_source.contains("\"/api/pending")
            && !client_source.contains("\"/api/registry/")
            && !client_source.contains("\"/api/remote/")
            && !client_source.contains("\"/api/ops/")
            && !client_source.contains("\"/api/sync/"),
        "frontend must not call legacy panel API routes"
    );
}

fn headers_with_host(host: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HOST,
        HeaderValue::from_str(host).expect("valid host header"),
    );
    headers
}

async fn read_panel_registry_status(
    addr: SocketAddr,
    host: &str,
    origin: Option<&str>,
) -> StatusCode {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect to test panel");
    let mut request =
        format!("GET /api/v1/registry/status HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(origin) = origin {
        request.push_str(&format!("Origin: {origin}\r\n"));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .expect("write panel request");

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("read panel response");
    let response = String::from_utf8_lossy(&response);
    let status_line = response.lines().next().expect("response status line");
    let status = status_line
        .split_whitespace()
        .nth(1)
        .expect("response status code")
        .parse::<u16>()
        .expect("numeric response status code");
    StatusCode::from_u16(status).expect("known status code")
}

#[test]
fn error_envelope_uses_expected_shape() {
    assert_eq!(
        error_envelope("skill.capture", "req-1", "INTERNAL_ERROR", "boom"),
        json!({
            "ok": false,
            "cmd": "skill.capture",
            "request_id": "req-1",
            "version": env!("CARGO_PKG_VERSION"),
            "data": {},
            "error": {
                "code": "INTERNAL_ERROR",
                "message": "boom",
                "details": {}
            },
            "meta": {
                "warnings": []
            }
        })
    );
}

#[test]
fn request_origin_matches_origin_or_referer() {
    let panel_origin = "http://127.0.0.1:43117";
    let mut headers = HeaderMap::new();
    headers.insert(ORIGIN, HeaderValue::from_static("http://127.0.0.1:43117"));
    assert!(request_origin_matches(panel_origin, &headers));

    let mut localhost_origin = HeaderMap::new();
    localhost_origin.insert(ORIGIN, HeaderValue::from_static("http://localhost:43117"));
    assert!(request_origin_matches(panel_origin, &localhost_origin));

    let mut referer_only = HeaderMap::new();
    referer_only.insert(
        REFERER,
        HeaderValue::from_static("http://127.0.0.1:43117/ops?x=1"),
    );
    assert!(request_origin_matches(panel_origin, &referer_only));

    let mut mismatched = HeaderMap::new();
    mismatched.insert(ORIGIN, HeaderValue::from_static("http://127.0.0.1:9999"));
    assert!(!request_origin_matches(panel_origin, &mismatched));

    let mut mixed = HeaderMap::new();
    mixed.insert(ORIGIN, HeaderValue::from_static("https://attacker.test"));
    mixed.insert(
        REFERER,
        HeaderValue::from_static("http://127.0.0.1:43117/ops?x=1"),
    );
    assert!(!request_origin_matches(panel_origin, &mixed));
}

#[test]
fn panel_request_authorization_requires_local_host_with_current_port() {
    let panel_origin = "http://127.0.0.1:43117";

    let local = headers_with_host("127.0.0.1:43117");
    assert!(panel_host_matches(panel_origin, &local));
    assert!(panel_request_authorized(panel_origin, &local));

    let localhost = headers_with_host("localhost:43117");
    assert!(panel_host_matches(panel_origin, &localhost));
    assert!(panel_request_authorized(panel_origin, &localhost));

    let hostile_host = headers_with_host("attacker.test:43117");
    assert!(!panel_request_authorized(panel_origin, &hostile_host));

    let wrong_port = headers_with_host("127.0.0.1:9999");
    assert!(!panel_request_authorized(panel_origin, &wrong_port));

    let mut hostile_origin = headers_with_host("127.0.0.1:43117");
    hostile_origin.insert(ORIGIN, HeaderValue::from_static("https://attacker.test"));
    assert!(!panel_request_authorized(panel_origin, &hostile_origin));
}

#[tokio::test]
async fn panel_read_route_validates_host_header() {
    let (root, mut state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);

    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .expect("bind test panel listener");
    let addr = listener.local_addr().expect("test panel listener address");
    state.panel_origin = format!("http://{}", addr);

    let app = crate::panel::panel_router(state);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve test panel router");
    });

    let hostile_host = format!("attacker.test:{}", addr.port());
    assert_eq!(
        read_panel_registry_status(addr, &hostile_host, None).await,
        StatusCode::FORBIDDEN
    );

    let local_host = format!("127.0.0.1:{}", addr.port());
    let local_origin = format!("http://{}", local_host);
    assert_eq!(
        read_panel_registry_status(addr, &local_host, Some(&local_origin)).await,
        StatusCode::OK
    );

    server.abort();
    cleanup_root(root);
}

#[test]
fn ensure_mutation_authorized_rejects_invalid_context_with_envelope() {
    let (root, state) = make_test_state();

    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
    let headers = HeaderMap::new();

    for cmd in MUTATION_COMMANDS {
        let response = ensure_mutation_authorized(&state, peer, &headers, cmd)
            .unwrap_or_else(|| panic!("guard should reject {cmd} without origin headers"));
        assert_eq!(response.0, StatusCode::FORBIDDEN, "{cmd} status");
        let Json(payload) = response.1;
        assert_eq!(payload["ok"], json!(false), "{cmd} ok");
        assert_eq!(payload["cmd"], json!(cmd), "{cmd} cmd");
        assert_eq!(
            payload["error"]["code"],
            json!("UNAUTHORIZED"),
            "{cmd} code"
        );
        assert!(payload["request_id"].as_str().is_some(), "{cmd} req id");
        assert!(payload.get("meta").is_some(), "{cmd} meta");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn run_panel_command_exposes_pending_queue_maintenance() {
    let (root, state) = make_test_state();

    for (cmd, command) in [
        (
            "ops.retry",
            Command::Ops {
                command: OpsCommand::Retry,
            },
        ),
        (
            "ops.purge",
            Command::Ops {
                command: OpsCommand::Purge,
            },
        ),
    ] {
        let (status, Json(payload)) = run_panel_command(&state, cmd, StatusCode::OK, command);
        assert_eq!(status, StatusCode::OK, "{cmd} status: {payload}");
        assert_eq!(payload["ok"], json!(true), "{cmd} ok");
        assert_eq!(payload["cmd"], json!(cmd), "{cmd} cmd");
        assert!(payload["request_id"].as_str().is_some(), "{cmd} req id");
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
fn run_panel_command_exposes_workspace_remote_set() {
    let (root, state) = make_test_state();
    let url = "https://example.com/loom-registry.git";

    let (status, Json(payload)) = run_panel_command(
        &state,
        "workspace.remote.set",
        StatusCode::OK,
        Command::Workspace {
            command: WorkspaceCommand::Remote {
                command: RemoteCommand::Set {
                    url: url.to_string(),
                },
            },
        },
    );

    assert_eq!(status, StatusCode::OK, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("workspace.remote"));
    assert_eq!(payload["data"]["remote"], json!("origin"));
    assert_eq!(payload["data"]["url"], json!(url));

    cleanup_root(root);
}

#[test]
fn run_panel_command_exposes_workspace_init() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) = run_panel_command(
        &state,
        "workspace.init",
        StatusCode::CREATED,
        Command::Workspace {
            command: WorkspaceCommand::Init(WorkspaceInitArgs {
                scan_existing: false,
            }),
        },
    );

    assert_eq!(status, StatusCode::CREATED, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("workspace.init"));
    assert_eq!(payload["data"]["initialized"], json!(true));
    assert_eq!(payload["data"]["scanned"], json!(false));
    assert_eq!(payload["meta"].get("op_id"), None);
    assert!(root.join("state/registry/schema.json").exists());

    cleanup_root(root);
}

#[test]
fn status_for_error_code_maps_lock_busy_to_conflict() {
    assert_eq!(
        status_for_error_code(Some("LOCK_BUSY")),
        StatusCode::CONFLICT
    );
    assert_eq!(
        status_for_error_code(Some("TARGET_NOT_FOUND")),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status_for_error_code(Some("ARG_INVALID")),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn registry_state_load_errors_map_to_observable_statuses() {
    assert_eq!(
        status_for_registry_state_load_error(Some("ARG_INVALID")),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        status_for_registry_state_load_error(Some("SCHEMA_MISMATCH")),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        status_for_registry_state_load_error(Some("STATE_CORRUPT")),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        status_for_registry_error_payload(&json!({
            "ok": false,
            "error": {"code": "ARG_INVALID", "message": "missing state"}
        })),
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn run_panel_command_returns_non_2xx_for_logical_failures_across_mutations() {
    let (root, state) = make_test_state();
    let cases = vec![
        (
            "target.add",
            StatusCode::CREATED,
            Command::Target {
                command: TargetCommand::Add(TargetAddArgs {
                    agent: crate::cli::AgentKind::Claude,
                    path: "relative/path".to_string(),
                    ownership: TargetOwnership::Managed,
                }),
            },
        ),
        (
            "target.remove",
            StatusCode::OK,
            Command::Target {
                command: TargetCommand::Remove(crate::cli::TargetShowArgs {
                    target_id: "missing".to_string(),
                }),
            },
        ),
        (
            "workspace.binding.add",
            StatusCode::CREATED,
            Command::Workspace {
                command: WorkspaceCommand::Binding {
                    command: WorkspaceBindingCommand::Add(BindingAddArgs {
                        agent: crate::cli::AgentKind::Claude,
                        profile: "default".to_string(),
                        matcher_kind: WorkspaceMatcherKind::PathPrefix,
                        matcher_value: "/tmp/x".to_string(),
                        target: "missing-target".to_string(),
                        policy_profile: "safe-capture".to_string(),
                    }),
                },
            },
        ),
        (
            "workspace.binding.remove",
            StatusCode::OK,
            Command::Workspace {
                command: WorkspaceCommand::Binding {
                    command: WorkspaceBindingCommand::Remove(crate::cli::BindingShowArgs {
                        binding_id: "missing-binding".to_string(),
                    }),
                },
            },
        ),
        (
            "skill.project",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Project(ProjectArgs {
                    skill: "missing-skill".to_string(),
                    binding: "missing-binding".to_string(),
                    target: None,
                    method: ProjectionMethod::Symlink,
                    dry_run: false,
                }),
            },
        ),
        (
            "skill.capture",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Capture(CaptureArgs {
                    skill: None,
                    binding: None,
                    instance: None,
                    message: None,
                    dry_run: false,
                }),
            },
        ),
        (
            "skill.save",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Save(SaveArgs {
                    skill: "missing-skill".to_string(),
                    message: None,
                }),
            },
        ),
        (
            "skill.snapshot",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Snapshot(SkillOnlyArgs {
                    skill: "missing-skill".to_string(),
                }),
            },
        ),
        (
            "skill.release",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Release(ReleaseArgs {
                    skill: "missing-skill".to_string(),
                    version: "v1".to_string(),
                }),
            },
        ),
        (
            "skill.rollback",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Rollback(RollbackArgs {
                    skill: "missing-skill".to_string(),
                    to: Some("HEAD~1".to_string()),
                    steps: None,
                    dry_run: false,
                }),
            },
        ),
        (
            "skill.trash.add",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Trash {
                    command: SkillTrashCommand::Add(SkillOnlyArgs {
                        skill: "missing-skill".to_string(),
                    }),
                },
            },
        ),
        (
            "skill.trash.restore",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Trash {
                    command: SkillTrashCommand::Restore(TrashRestoreArgs {
                        skill: "missing-skill".to_string(),
                        trash_id: Some("missing-trash".to_string()),
                    }),
                },
            },
        ),
        (
            "skill.trash.purge",
            StatusCode::OK,
            Command::Skill {
                command: SkillCommand::Trash {
                    command: SkillTrashCommand::Purge(TrashPurgeArgs {
                        trash_id: "missing-trash".to_string(),
                    }),
                },
            },
        ),
        (
            "sync.push",
            StatusCode::OK,
            Command::Sync {
                command: SyncCommand::Push(crate::cli::SyncPushArgs { dry_run: false }),
            },
        ),
        (
            "sync.pull",
            StatusCode::OK,
            Command::Sync {
                command: SyncCommand::Pull,
            },
        ),
    ];

    for (cmd, success_status, command) in cases {
        let (status, Json(payload)) = run_panel_command(&state, cmd, success_status, command);
        assert!(
            !status.is_success(),
            "expected non-2xx for {cmd}, got {status}"
        );
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["cmd"], json!(cmd));
        assert!(payload["request_id"].as_str().is_some());
        assert!(payload["error"]["code"].as_str().is_some());
        assert!(payload["error"]["message"].as_str().is_some());
        assert!(payload.get("meta").is_some());
    }

    let _ = fs::remove_dir_all(root);
}
