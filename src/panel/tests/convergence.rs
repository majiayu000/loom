use super::*;
use crate::panel::ConvergencePlanRequest;
use crate::panel::handlers::{registry_convergence_plan, v1_health};
use crate::state_model::REGISTRY_SCHEMA_VERSION;
use axum::{
    Json,
    extract::{ConnectInfo, Path as AxumPath, State},
    http::StatusCode,
};
use serde_json::{Value, json};

fn plan_request(accept_restart_required: bool, require_runtime: bool) -> ConvergencePlanRequest {
    ConvergencePlanRequest {
        agent: None,
        workspace: None,
        profile: None,
        require_runtime,
        accept_restart_required,
        push_remote: false,
    }
}

#[tokio::test]
async fn v1_health_returns_cli_envelope_shape() {
    let (status, Json(payload)) = v1_health().await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("panel.health"));
    assert_eq!(payload["error"], Value::Null);
    assert_eq!(payload["data"]["service"], json!("loom-panel"));
    assert_eq!(
        payload["data"]["capabilities"]["skill_convergence"],
        json!({
            "plan": true,
            "apply": true,
            "requires_plan_digest": true,
            "remote_last": true
        })
    );
    assert_eq!(payload["meta"]["warnings"], json!([]));
}

#[tokio::test]
async fn convergence_plan_rejects_restart_acceptance_without_runtime_requirement() {
    let (root, state) = make_test_state();
    let (status, Json(payload)) = registry_convergence_plan(
        AxumPath("demo".to_string()),
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
        Json(plan_request(true, false)),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{payload}");
    assert_eq!(payload["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(
        payload["error"]["message"],
        json!("accept_restart_required requires require_runtime")
    );
    assert!(!root.join("state/events/commands.jsonl").exists());
    cleanup_root(root);
}

#[tokio::test]
async fn convergence_plan_route_returns_reviewable_digest_without_applying() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let skill_dir = root.join("skills/demo");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo convergence route.\n---\n# Demo\n",
    )
    .expect("write skill");
    git_ok(&root, &["init"]);
    git_ok(&root, &["config", "user.email", "panel@example.com"]);
    git_ok(&root, &["config", "user.name", "Panel Test"]);
    git_ok(&root, &["add", "."]);
    git_ok(&root, &["commit", "-m", "fixture"]);
    let before = fs::read_to_string(skill_dir.join("SKILL.md")).expect("read source before plan");

    let (status, Json(payload)) = registry_convergence_plan(
        AxumPath("demo".to_string()),
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
        Json(plan_request(false, false)),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{payload}");
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("plan.converge"));
    assert_eq!(payload["data"]["execution_enabled"], json!(true));
    assert_eq!(payload["data"]["safe_to_apply"], json!(true));
    assert_eq!(payload["data"]["requires_digest_confirmation"], json!(true));
    assert!(payload["data"]["plan_id"].is_string());
    assert!(payload["data"]["plan_digest"].is_string());
    assert_eq!(
        fs::read_to_string(skill_dir.join("SKILL.md")).expect("read source after plan"),
        before,
        "planning must not mutate the skill source"
    );

    cleanup_root(root);
}
