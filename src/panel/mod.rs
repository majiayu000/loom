mod auth;
mod handlers;
mod skill_diff;
mod skill_history;
mod static_serve;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
};
use serde::Deserialize;

use crate::cli::{AgentKind, ProjectionMethod, TargetOwnership, WorkspaceMatcherKind};
use crate::state::AppContext;

use handlers::*;
use skill_diff::v3_skill_diff;
use skill_history::v3_skill_history;
use static_serve::{ensure_panel_dist, frontend_index, frontend_static_asset};

#[derive(Clone)]
pub(crate) struct PanelState {
    pub(crate) ctx: Arc<AppContext>,
    pub(crate) panel_origin: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TargetAddRequest {
    pub(super) agent: AgentKind,
    pub(super) path: String,
    #[serde(default)]
    pub(super) ownership: Option<TargetOwnership>,
}

#[derive(Debug, Deserialize)]
pub(super) struct BindingAddRequest {
    pub(super) agent: AgentKind,
    pub(super) profile: String,
    pub(super) matcher_kind: WorkspaceMatcherKind,
    pub(super) matcher_value: String,
    pub(super) target: String,
    #[serde(default)]
    pub(super) policy_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ProjectRequest {
    pub(super) skill: String,
    pub(super) binding: String,
    #[serde(default)]
    pub(super) target: Option<String>,
    #[serde(default)]
    pub(super) method: Option<ProjectionMethod>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CaptureRequest {
    #[serde(default)]
    pub(super) skill: Option<String>,
    #[serde(default)]
    pub(super) binding: Option<String>,
    #[serde(default)]
    pub(super) instance: Option<String>,
    #[serde(default)]
    pub(super) message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct DiffParams {
    #[serde(default)]
    pub(super) rev_a: Option<String>,
    #[serde(default)]
    pub(super) rev_b: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct HistoryRepairRequest {
    pub(super) strategy: String,
}

pub async fn run_panel(ctx: AppContext, port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    ensure_panel_dist()?;

    let state = PanelState {
        ctx: Arc::new(ctx),
        panel_origin: format!("http://{}", addr),
    };

    let app = Router::new()
        .route("/", get(frontend_index))
        .route("/api/health", get(health))
        .route("/api/info", get(info))
        .route("/api/workspace/status", get(workspace_status))
        .route("/api/skills", get(skills))
        .route("/api/v3/status", get(v3_status))
        .route("/api/v3/bindings", get(v3_bindings))
        .route("/api/v3/bindings/{binding_id}", get(v3_binding_show))
        .route("/api/v3/targets", get(v3_targets))
        .route("/api/v3/targets/{target_id}", get(v3_target_show))
        .route("/api/v3/targets", post(v3_target_add))
        .route("/api/v3/targets/{target_id}/remove", post(v3_target_remove))
        .route("/api/v3/bindings", post(v3_binding_add))
        .route(
            "/api/v3/bindings/{binding_id}/remove",
            post(v3_binding_remove),
        )
        .route("/api/v3/project", post(v3_project))
        .route("/api/v3/capture", post(v3_capture))
        .route("/api/v3/skills/{skill_name}/diff", get(v3_skill_diff))
        .route("/api/v3/skills/{skill_name}/history", get(v3_skill_history))
        .route("/api/remote/status", get(remote_status))
        .route("/api/pending", get(pending))
        .route("/api/ops/history/list", get(ops_history_list))
        .route("/api/ops/history/diagnose", get(ops_history_diagnose))
        .route("/api/ops/retry", post(ops_retry))
        .route("/api/ops/purge", post(ops_purge))
        .route("/api/ops/history/repair", post(ops_history_repair))
        .route("/api/sync/push", post(sync_push))
        .route("/api/sync/pull", post(sync_pull))
        .route("/api/sync/replay", post(sync_replay))
        .route("/{*path}", get(frontend_static_asset))
        .with_state(state);

    println!("panel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PanelState,
        auth::{
            ensure_mutation_authorized, error_envelope, request_origin_matches, run_panel_command,
            status_for_error_code, status_for_v3_error_payload, status_for_v3_state_load_error,
        },
        handlers::{remote_status, v3_status},
        static_serve::{content_type_for, resolve_panel_asset_path},
    };
    use crate::cli::{
        BindingAddArgs, CaptureArgs, Command, ProjectArgs, ProjectionMethod, SkillCommand,
        SyncCommand, TargetAddArgs, TargetCommand, TargetOwnership, WorkspaceBindingCommand,
        WorkspaceCommand, WorkspaceMatcherKind,
    };
    use crate::state::AppContext;
    use crate::state_model::{
        V3_SCHEMA_VERSION, V3BindingsFile, V3OpsCheckpoint, V3ProjectionsFile, V3RulesFile,
        V3SchemaFile, V3StatePaths, V3TargetsFile,
    };
    use axum::{
        Json,
        extract::State,
        http::{HeaderMap, HeaderValue, StatusCode},
    };
    use chrono::Utc;
    use serde_json::json;
    use std::{
        fs,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::Path,
        sync::Arc,
    };
    use uuid::Uuid;

    fn make_test_state() -> (std::path::PathBuf, PanelState) {
        let root = std::env::temp_dir().join(format!("loom-panel-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create panel test root");
        let ctx = AppContext::new(Some(root.clone())).expect("build app context");
        let state = PanelState {
            ctx: Arc::new(ctx),
            panel_origin: "http://127.0.0.1:43117".to_string(),
        };
        (root, state)
    }

    fn write_v3_snapshot(root: &Path, schema_version: u32) {
        let paths = V3StatePaths::from_root(root);
        fs::create_dir_all(&paths.v3_dir).expect("create v3 dir");
        fs::create_dir_all(&paths.ops_dir).expect("create v3 ops dir");
        fs::create_dir_all(&paths.observations_dir).expect("create v3 observations dir");
        let now = Utc::now();

        fs::write(
            &paths.schema_file,
            serde_json::to_vec_pretty(&V3SchemaFile {
                schema_version,
                created_at: now,
                writer: "loom-test".to_string(),
            })
            .expect("serialize schema"),
        )
        .expect("write schema");
        fs::write(
            &paths.targets_file,
            serde_json::to_vec_pretty(&V3TargetsFile {
                schema_version,
                targets: Vec::new(),
            })
            .expect("serialize targets"),
        )
        .expect("write targets");
        fs::write(
            &paths.bindings_file,
            serde_json::to_vec_pretty(&V3BindingsFile {
                schema_version,
                bindings: Vec::new(),
            })
            .expect("serialize bindings"),
        )
        .expect("write bindings");
        fs::write(
            &paths.rules_file,
            serde_json::to_vec_pretty(&V3RulesFile {
                schema_version,
                rules: Vec::new(),
            })
            .expect("serialize rules"),
        )
        .expect("write rules");
        fs::write(
            &paths.projections_file,
            serde_json::to_vec_pretty(&V3ProjectionsFile {
                schema_version,
                projections: Vec::new(),
            })
            .expect("serialize projections"),
        )
        .expect("write projections");
        fs::write(&paths.operations_file, []).expect("write operations");
        fs::write(
            &paths.checkpoint_file,
            serde_json::to_vec_pretty(&V3OpsCheckpoint {
                schema_version,
                last_scanned_op_id: None,
                last_acked_op_id: None,
                updated_at: now,
            })
            .expect("serialize checkpoint"),
        )
        .expect("write checkpoint");
    }

    async fn run_v3_status(state: PanelState) -> (StatusCode, serde_json::Value) {
        let (status, Json(payload)) = v3_status(State(state)).await;
        (status, payload)
    }

    fn status_code(payload: &serde_json::Value) -> Option<&str> {
        payload
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_str)
    }

    fn cleanup_root(root: std::path::PathBuf) {
        let _ = fs::remove_dir_all(root);
    }

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

        // Covers every mutation surface the panel exposes, including the
        // new sync routes. Without origin headers they must all return
        // UNAUTHORIZED so writes cannot be driven from an untrusted origin.
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

    #[tokio::test]
    async fn v3_status_returns_bad_request_when_state_is_missing() {
        let (root, state) = make_test_state();

        let (status, payload) = run_v3_status(state).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(status_code(&payload), Some("ARG_INVALID"));
        assert!(
            payload["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("v3 state not initialized"))
        );

        cleanup_root(root);
    }

    #[tokio::test]
    async fn v3_status_returns_internal_error_when_state_is_corrupt() {
        let (root, state) = make_test_state();
        let paths = V3StatePaths::from_root(&root);
        fs::create_dir_all(&paths.v3_dir).expect("create v3 dir");
        fs::write(&paths.schema_file, b"{not-json").expect("write corrupt schema");

        let (status, payload) = run_v3_status(state).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(status_code(&payload), Some("STATE_CORRUPT"));
        assert!(payload["error"]["message"].as_str().is_some());

        cleanup_root(root);
    }

    #[tokio::test]
    async fn v3_status_returns_internal_error_when_schema_mismatches() {
        let (root, state) = make_test_state();
        write_v3_snapshot(&root, V3_SCHEMA_VERSION + 1);

        let (status, payload) = run_v3_status(state).await;

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
    async fn v3_status_returns_ok_when_snapshot_loads() {
        let (root, state) = make_test_state();
        write_v3_snapshot(&root, V3_SCHEMA_VERSION);

        let (status, payload) = run_v3_status(state).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["ok"], json!(true));
        assert_eq!(payload["data"]["schema_version"], json!(V3_SCHEMA_VERSION));
        assert_eq!(payload["data"]["counts"]["targets"], json!(0));
        assert_eq!(payload["data"]["counts"]["bindings"], json!(0));

        cleanup_root(root);
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
            // Note: sync.push / sync.pull need a configured remote, so on a
            // fresh workspace they correctly return an ARG_INVALID (non-2xx)
            // envelope. Covered separately in
            // `sync_routes_without_remote_return_arg_invalid` so the happy
            // path of `sync.replay` (which is a valid no-op on empty state)
            // is not conflated with logical-failure coverage.
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
        assert_eq!(payload["remote"]["configured"], json!(false));
        assert!(payload["remote"].is_object());
        assert!(payload["warnings"].is_array());

        let _ = fs::remove_dir_all(root);
    }
}
