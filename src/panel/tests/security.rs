use super::*;
use crate::cli::{
    BindingAddArgs, CaptureArgs, Command, OpsCommand, ProjectArgs, ProjectionMethod, RemoteCommand,
    SkillCommand, SyncCommand, TargetAddArgs, TargetCommand, TargetOwnership,
    WorkspaceBindingCommand, WorkspaceCommand, WorkspaceMatcherKind,
};
use crate::panel::auth::{
    ensure_mutation_authorized, error_envelope, request_origin_matches, run_panel_command,
    status_for_error_code, status_for_v3_error_payload, status_for_v3_state_load_error,
};
use axum::http::{HeaderMap, HeaderValue};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

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
    headers.insert("origin", HeaderValue::from_static("http://127.0.0.1:43117"));
    assert!(request_origin_matches(panel_origin, &headers));

    let mut referer_only = HeaderMap::new();
    referer_only.insert(
        "referer",
        HeaderValue::from_static("http://127.0.0.1:43117/ops?x=1"),
    );
    assert!(request_origin_matches(panel_origin, &referer_only));

    let mut mismatched = HeaderMap::new();
    mismatched.insert("origin", HeaderValue::from_static("http://127.0.0.1:9999"));
    assert!(!request_origin_matches(panel_origin, &mismatched));
}

#[test]
fn ensure_mutation_authorized_rejects_invalid_context_with_envelope() {
    let (root, state) = make_test_state();

    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000);
    let headers = HeaderMap::new();

    for cmd in [
        "target.add",
        "target.remove",
        "workspace.binding.add",
        "workspace.binding.remove",
        "skill.project",
        "skill.capture",
        "ops.retry",
        "ops.purge",
        "ops.history.repair",
        "workspace.remote.set",
        "sync.push",
        "sync.pull",
        "sync.replay",
    ] {
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
fn v3_state_load_errors_map_to_observable_statuses() {
    assert_eq!(
        status_for_v3_state_load_error(Some("ARG_INVALID")),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        status_for_v3_state_load_error(Some("SCHEMA_MISMATCH")),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        status_for_v3_state_load_error(Some("STATE_CORRUPT")),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        status_for_v3_error_payload(&json!({
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
                }),
            },
        ),
        (
            "sync.push",
            StatusCode::OK,
            Command::Sync {
                command: SyncCommand::Push,
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
