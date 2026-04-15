mod auth;
mod handlers;
mod static_serve;

use std::net::SocketAddr;
use std::path::PathBuf;
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
use static_serve::{ensure_panel_dist, frontend_index, frontend_static_asset};

#[derive(Clone)]
pub(crate) struct PanelState {
    pub(crate) ctx: Arc<AppContext>,
    pub(crate) dist_dir: PathBuf,
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

pub async fn run_panel(ctx: AppContext, port: u16) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let dist_dir = ctx.root.join("panel/dist");
    ensure_panel_dist(&dist_dir)?;

    let state = PanelState {
        ctx: Arc::new(ctx),
        dist_dir,
        panel_origin: format!("http://{}", addr),
    };

    let app = Router::new()
        .route("/", get(frontend_index))
        .route("/api/health", get(health))
        .route("/api/info", get(info))
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
        .route("/api/remote/status", get(remote_status))
        .route("/api/pending", get(pending))
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
            status_for_error_code,
        },
        static_serve::{content_type_for, resolve_panel_asset_path},
    };
    use crate::cli::{
        BindingAddArgs, CaptureArgs, Command, ProjectArgs, ProjectionMethod, SkillCommand,
        TargetAddArgs, TargetCommand, TargetOwnership, WorkspaceBindingCommand, WorkspaceCommand,
        WorkspaceMatcherKind,
    };
    use crate::state::AppContext;
    use axum::{
        Json,
        http::{HeaderMap, HeaderValue, StatusCode},
    };
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
            dist_dir: root.join("panel/dist"),
            panel_origin: "http://127.0.0.1:43117".to_string(),
        };
        (root, state)
    }

    #[test]
    fn resolve_panel_asset_path_rejects_invalid_components() {
        let dist_dir = Path::new("/tmp/panel-dist");

        assert_eq!(
            resolve_panel_asset_path(dist_dir, "assets/index.js"),
            Some(dist_dir.join("assets/index.js"))
        );
        assert_eq!(
            resolve_panel_asset_path(dist_dir, "./assets/index.css"),
            Some(dist_dir.join("assets/index.css"))
        );
        assert_eq!(resolve_panel_asset_path(dist_dir, "../secret.txt"), None);
        assert_eq!(resolve_panel_asset_path(dist_dir, "/etc/passwd"), None);
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
        let response = ensure_mutation_authorized(&state, peer, &headers, "target.add")
            .expect("guard should reject request without origin headers");
        assert_eq!(response.0, StatusCode::FORBIDDEN);
        let Json(payload) = response.1;
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["cmd"], json!("target.add"));
        assert_eq!(payload["error"]["code"], json!("UNAUTHORIZED"));
        assert!(payload["request_id"].as_str().is_some());
        assert!(payload.get("meta").is_some());

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
}
