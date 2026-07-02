mod common;

use std::fs;
use std::path::Path;

use common::{TestDir, run_loom, write_file};
use serde_json::{Value, json};

fn test_actor() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-local-actor".to_string())
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json")
}

fn write_roles(root: &Path, grants: Value) {
    write_file(
        &root.join("state/registry/roles.json"),
        &(serde_json::to_string_pretty(&json!({
            "schema_version": 1,
            "grants": grants,
        }))
        .expect("serialize roles")
            + "\n"),
    );
}

fn grant(subject: &str, role: &str) -> Value {
    json!({
        "subject": subject,
        "role": role,
        "granted_at": "2026-07-01T00:00:00Z",
        "granted_by": "test",
    })
}

#[test]
fn org_policy_init_show_roles_and_check_decisions() {
    let root = TestDir::new("org-policy-init");
    let actor = test_actor();

    let (output, env) = run_loom(root.path(), &["policy", "org", "init"]);
    assert!(
        !output.status.success(),
        "fresh init without admin must fail: {env}"
    );
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));

    let (output, env) = run_loom(
        root.path(),
        &["policy", "org", "init", "--bootstrap-admin", &actor],
    );
    assert!(output.status.success(), "policy init should pass: {env}");
    assert_eq!(env["data"]["created"], json!(true));
    assert!(root.path().join("state/registry/org_policy.toml").exists());
    assert!(root.path().join("state/registry/roles.json").exists());

    let (output, show) = run_loom(root.path(), &["policy", "org", "show"]);
    assert!(output.status.success(), "policy show should pass: {show}");
    assert_eq!(show["data"]["policy"]["schema"], json!("loom.policy.v1"));
    assert_eq!(
        show["data"]["roles"]["by_subject"][&actor][0],
        json!("admin")
    );

    let (output, roles) = run_loom(root.path(), &["roles", "list"]);
    assert!(output.status.success(), "roles list should pass: {roles}");
    assert_eq!(roles["data"]["current_actor"], json!(actor));

    let (output, check) = run_loom(
        root.path(),
        &[
            "policy",
            "org",
            "check",
            "skill.activate",
            "--skill",
            "demo",
        ],
    );
    assert!(output.status.success(), "policy check should pass: {check}");
    assert_eq!(check["data"]["policy"]["decision"], json!("allow"));
    assert_eq!(
        check["data"]["policy"]["required_roles"],
        json!(["reviewer"])
    );

    let (output, alias) = run_loom(root.path(), &["policy", "org", "check", "workspace.remote"]);
    assert!(output.status.success(), "alias check should pass: {alias}");
    assert_eq!(
        alias["data"]["policy"]["action"],
        json!("workspace.remote.set")
    );

    let (output, second) = run_loom(
        root.path(),
        &[
            "policy",
            "org",
            "init",
            "--bootstrap-admin",
            "somebody-else",
        ],
    );
    assert!(
        output.status.success(),
        "idempotent init should pass: {second}"
    );
    assert_eq!(second["data"]["created"], json!(false));
    assert!(
        second["data"]["roles"]["by_subject"]["somebody-else"].is_null(),
        "init must not reset or add another admin: {second}"
    );
}

#[test]
fn approval_requests_are_append_only_and_role_checked() {
    let root = TestDir::new("org-policy-approvals");
    let actor = test_actor();
    let bootstrap = format!("bootstrap-{actor}");
    let (output, env) = run_loom(
        root.path(),
        &["policy", "org", "init", "--bootstrap-admin", &bootstrap],
    );
    assert!(output.status.success(), "policy init should pass: {env}");

    let (output, request) = run_loom(
        root.path(),
        &[
            "approval",
            "request",
            "skill.activate",
            "--skill",
            "demo",
            "--reason",
            "please review token=secret123",
        ],
    );
    assert!(
        output.status.success(),
        "approval request should pass: {request}"
    );
    assert_eq!(
        request["data"]["policy"]["decision"],
        json!("approval_required")
    );
    assert_eq!(
        request["data"]["request"]["required_approvals"],
        json!(["approval:reviewer"])
    );
    assert!(
        !request["data"]["request"]["reason_redacted"]
            .as_str()
            .unwrap_or_default()
            .contains("secret123"),
        "request reason must be redacted: {request}"
    );
    let request_id = request["data"]["request"]["request_id"]
        .as_str()
        .expect("request id")
        .to_string();
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/approvals.jsonl"))
            .expect("read approvals")
            .lines()
            .count(),
        1
    );

    let (output, denied) = run_loom(root.path(), &["approval", "approve", &request_id]);
    assert!(
        !output.status.success(),
        "approve without role must fail: {denied}"
    );
    assert_eq!(denied["error"]["code"], json!("POLICY_BLOCKED"));

    write_roles(
        root.path(),
        json!([grant(&bootstrap, "admin"), grant(&actor, "reviewer"),]),
    );
    let (output, approved) = run_loom(
        root.path(),
        &[
            "approval",
            "approve",
            &request_id,
            "--comment",
            "ok password=secret456",
        ],
    );
    assert!(output.status.success(), "approve should pass: {approved}");
    assert_eq!(approved["data"]["status"], json!("approved"));

    let (output, list) = run_loom(root.path(), &["approval", "list", "--approved"]);
    assert!(output.status.success(), "approval list should pass: {list}");
    assert_eq!(list["data"]["count"], json!(1));
    assert_eq!(list["data"]["requests"][0]["status"], json!("approved"));
    assert!(
        !fs::read_to_string(root.path().join("state/registry/approvals.jsonl"))
            .expect("read approvals")
            .contains("secret456"),
        "decision comments must be redacted"
    );

    let (output, second) = run_loom(
        root.path(),
        &[
            "approval",
            "request",
            "provider.remove",
            "--provider",
            "corp",
        ],
    );
    assert!(
        output.status.success(),
        "provider request should pass: {second}"
    );
    let second_id = second["data"]["request"]["request_id"]
        .as_str()
        .expect("second request id")
        .to_string();
    write_roles(
        root.path(),
        json!([grant(&bootstrap, "admin"), grant(&actor, "maintainer"),]),
    );
    let (output, rejected) = run_loom(root.path(), &["approval", "reject", &second_id]);
    assert!(output.status.success(), "reject should pass: {rejected}");
    assert_eq!(rejected["data"]["status"], json!("rejected"));
}

#[test]
fn blocked_skill_denies_and_malformed_policy_fails_closed() {
    let root = TestDir::new("org-policy-deny");
    let actor = test_actor();
    let (output, env) = run_loom(
        root.path(),
        &["policy", "org", "init", "--bootstrap-admin", &actor],
    );
    assert!(output.status.success(), "policy init should pass: {env}");

    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"danger","trust":"blocked","quarantined":false,"reason":"test","updated_at":"2026-07-01T00:00:00Z","updated_by":"test"}]}
"#,
    );
    let (output, check) = run_loom(
        root.path(),
        &[
            "policy",
            "org",
            "check",
            "skill.activate",
            "--skill",
            "danger",
        ],
    );
    assert!(output.status.success(), "policy check should pass: {check}");
    assert_eq!(check["data"]["policy"]["decision"], json!("deny"));

    let before_roles = read_json(&root.path().join("state/registry/roles.json"));
    write_file(
        &root.path().join("state/registry/org_policy.toml"),
        "schema = [not valid]\n",
    );
    let (output, env) = run_loom(root.path(), &["policy", "org", "show"]);
    assert!(
        !output.status.success(),
        "malformed policy must fail closed: {env}"
    );
    assert_eq!(env["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        read_json(&root.path().join("state/registry/roles.json")),
        before_roles,
        "failed policy reads must not rewrite role state"
    );
}
