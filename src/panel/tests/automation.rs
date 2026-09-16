use super::*;
use crate::{
    cli::Cli,
    panel::{automation_request::AutomationRequest, handlers::automation_execute},
};
use axum::extract::ConnectInfo;
use clap::Parser;
use serde_json::{Value, json};

#[test]
fn automation_requests_parse_as_existing_cli_commands() {
    let cases = [
        json!({"action":"workflow_create","name":"demo","skillset":"bundle","dry_run":true}),
        json!({"action":"workflow_plan","name":"demo","workspace":"/tmp/work"}),
        json!({"action":"workflow_apply","plan":"p1","inputs":"/tmp/inputs.json","approvals":["approval:test"],"idempotency_key":"k1","dry_run":true}),
        json!({"action":"skillset_eval","name":"demo","runner":"codex-cli","baseline":"no-skill","dry_run":true}),
        json!({"action":"author_draft","name":"demo","session":"/tmp/session.txt","dry_run":true}),
        json!({"action":"author_rewrite","name":"demo","instruction":"--root=/evil","dry_run":true}),
        json!({"action":"author_apply","patch":"p1","idempotency_key":"k1"}),
        json!({"action":"instruction_scan","workspace":"/tmp/work"}),
        json!({"action":"instruction_migrate","instruction_id":"i1","workspace":"/tmp/work","name":"demo","target":"skill","dry_run":true}),
        json!({"action":"package_plan","source":"skill:demo","format":"npm","output":"/tmp/plan.json"}),
        json!({"action":"package_build","plan":"/tmp/plan.json","output":"/tmp/package.tgz","idempotency_key":"k1"}),
        json!({"action":"package_verify","artifact":"/tmp/package.tgz"}),
        json!({"action":"provision_plan","target":"remote","workspace":"/tmp/work"}),
        json!({"action":"provision_export","plan":"p1","format":"tar","output":"/tmp/config.tar"}),
        json!({"action":"provision_import","artifact":"/tmp/config.tar","output":"/tmp/new-directory","dry_run":true}),
        json!({"action":"provision_apply","plan":"p1","approvals":["approval:test"],"idempotency_key":"k1"}),
        json!({"action":"catalog_search","provider":"github","query":"a; echo b"}),
        json!({"action":"catalog_preview","locator":"github:acme/demo"}),
    ];
    for value in cases {
        let req: AutomationRequest = serde_json::from_value(value.clone()).unwrap();
        let cli = Cli::try_parse_from(std::iter::once("loom".to_string()).chain(req.argv()))
            .unwrap_or_else(|err| panic!("{value}: {err}"));
        assert!(cli.root.is_none());
    }
}

#[tokio::test]
async fn automation_requires_local_origin_and_returns_real_cli_errors() {
    let (root, state) = make_test_state();
    let request = || {
        serde_json::from_value::<AutomationRequest>(
            json!({"action":"package_verify","artifact":"/missing-package.tar"}),
        )
        .unwrap()
    };
    let (status, Json(body)) = automation_execute(
        ConnectInfo(panel_peer()),
        HeaderMap::new(),
        State(state.clone()),
        Json(request()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");
    let (status, Json(body)) = automation_execute(
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
        Json(request()),
    )
    .await;
    assert_ne!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert_eq!(body["cmd"], "package.verify");
    assert_ne!(body["error"], Value::Null);
    cleanup_root(root);
}

#[tokio::test]
async fn automation_preview_uses_fixed_registry_without_writing_patch() {
    let (root, state) = make_test_state();
    fs::create_dir_all(root.join("skills/demo")).unwrap();
    fs::write(
        root.join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo workflow.\n---\n# Demo\n",
    )
    .unwrap();
    let req=serde_json::from_value(json!({"action":"author_rewrite","name":"demo","instruction":"Clarify examples","dry_run":true})).unwrap();
    let (status, Json(body)) = automation_execute(
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
        Json(req),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["provider"], "codex-cli");
    assert_eq!(body["data"]["artifact_written"], false);
    assert!(!root.join("state/patches").exists());
    cleanup_root(root);
}
