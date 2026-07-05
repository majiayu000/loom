mod common;

use std::{fs, process::Command};

use serde_json::{Value, json};

use common::{TestDir, run_loom, write_file, write_skill};

fn write_demo_skills(root: &TestDir) {
    write_skill(
        root.path(),
        "review-helper",
        "---\nname: review-helper\ndescription: Use when reviewing workflow plans.\n---\n# Review helper\n",
    );
    write_skill(
        root.path(),
        "test-writer",
        "---\nname: test-writer\ndescription: Use when writing focused workflow tests.\n---\n# Test writer\n",
    );
}

fn write_workflow(root: &TestDir, name: &str, body: &str) -> String {
    let path = root.path().join(format!("{name}.json"));
    write_file(&path, body);
    path.display().to_string()
}

fn review_workflow_json() -> &'static str {
    r#"{
  "workflow_id": "review-flow",
  "description": "Review workflow",
  "external_inputs": ["task"],
  "nodes": [
    {
      "id": "orient",
      "skill_id": "review-helper",
      "kind": "skill",
      "requires": ["task"],
      "outputs": ["plan"]
    },
    {
      "id": "test",
      "skill_id": "test-writer",
      "kind": "skill",
      "requires": ["plan"],
      "outputs": ["tests"],
      "mutates_workspace": true
    }
  ],
  "edges": [
    {"from": "orient", "to": "test"}
  ],
  "policy": {
    "max_nodes": 8,
    "max_depth": 6,
    "requires_human_approval_before": ["test"],
    "rollback_strategy": "checkpoint-before-mutating-node"
  }
}
"#
}

fn create_review_workflow(root: &TestDir) -> Value {
    let workflow = write_workflow(root, "review-flow", review_workflow_json());
    let (output, env) = run_loom(
        root.path(),
        &["workflow", "create", "review-flow", "--file", &workflow],
    );
    assert!(
        output.status.success(),
        "workflow create should pass: {env}"
    );
    env
}

#[test]
fn workflow_create_show_plan_and_preflight_are_guarded() {
    let root = TestDir::new("workflow-guarded-plan");
    let workspace = TestDir::new("workflow-workspace");
    write_demo_skills(&root);

    let env = create_review_workflow(&root);
    assert_eq!(env["cmd"], json!("workflow.create"));
    assert_eq!(env["data"]["workflow"]["workflow_id"], json!("review-flow"));
    assert_eq!(
        env["data"]["workflow"]["ordered_node_ids"],
        json!(["orient", "test"])
    );
    assert!(env["data"]["commit"].as_str().is_some());
    assert!(root.path().join("state/registry/workflows.json").is_file());

    let (output, env) = run_loom(root.path(), &["workflow", "show", "review-flow"]);
    assert!(output.status.success(), "workflow show should pass: {env}");
    assert_eq!(env["cmd"], json!("workflow.show"));
    assert_eq!(env["data"]["ordered_node_ids"], json!(["orient", "test"]));

    let workspace_arg = workspace.path().display().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "plan",
            "review-flow",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(output.status.success(), "workflow plan should pass: {env}");
    assert_eq!(env["cmd"], json!("workflow.plan"));
    assert_eq!(env["data"]["schema_version"], json!("workflow-plan-v1"));
    assert_eq!(env["data"]["operation"], json!("workflow"));
    assert_eq!(env["data"]["safe_to_run"], json!(false));
    assert_eq!(env["data"]["ready"], json!(false));
    assert!(
        env["data"]["activation_steps"]
            .as_array()
            .expect("activation steps")
            .iter()
            .any(|step| step["skill"] == json!("review-helper")),
        "plan should require explicit active-view work: {env}"
    );
    assert!(
        env["data"]["required_approvals"]
            .as_array()
            .expect("approvals")
            .contains(&json!("approve-test")),
        "mutating node approval should be explicit: {env}"
    );
    let plan_id = env["data"]["plan_id"]
        .as_str()
        .expect("plan id")
        .to_string();
    assert!(
        root.path()
            .join("state/registry/workflow_plans.json")
            .is_file()
    );

    let (output, env) = run_loom(root.path(), &["workflow", "preflight", &plan_id]);
    assert!(output.status.success(), "preflight should pass: {env}");
    assert_eq!(env["cmd"], json!("workflow.preflight"));
    assert_eq!(env["data"]["valid"], json!(true));
    assert_eq!(env["data"]["safe_to_run"], json!(false));
}

#[test]
fn workflow_create_rejects_cycles() {
    let root = TestDir::new("workflow-cycle");
    let workflow = write_workflow(
        &root,
        "cycle-flow",
        r#"{
  "workflow_id": "cycle-flow",
  "nodes": [
    {"id": "a", "skill_id": "review-helper"},
    {"id": "b", "skill_id": "test-writer"}
  ],
  "edges": [
    {"from": "a", "to": "b"},
    {"from": "b", "to": "a"}
  ]
}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &["workflow", "create", "cycle-flow", "--file", &workflow],
    );
    assert!(!output.status.success(), "cycle should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(
        env["error"]["details"]["validation_code"],
        json!("CYCLE_DETECTED")
    );
    assert!(!root.path().join("state/registry/workflows.json").exists());
}

#[test]
fn workflow_plan_rejects_missing_skill_sources() {
    let root = TestDir::new("workflow-missing-skill");
    let workspace = TestDir::new("workflow-missing-skill-workspace");
    let workflow = write_workflow(
        &root,
        "missing-flow",
        r#"{
  "workflow_id": "missing-flow",
  "nodes": [
    {"id": "missing", "skill_id": "missing-skill"}
  ]
}
"#,
    );
    let (output, env) = run_loom(
        root.path(),
        &["workflow", "create", "missing-flow", "--file", &workflow],
    );
    assert!(
        output.status.success(),
        "create only validates workflow structure: {env}"
    );

    let workspace_arg = workspace.path().display().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "plan",
            "missing-flow",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(!output.status.success(), "missing skill should fail");
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));
}

#[test]
fn workflow_plan_rejects_blocked_or_quarantined_skills() {
    let root = TestDir::new("workflow-blocked-skill");
    let workspace = TestDir::new("workflow-blocked-workspace");
    write_demo_skills(&root);
    create_review_workflow(&root);
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"review-helper","trust":"blocked","quarantined":false,"reason":"blocked by test","updated_at":"2026-07-01T00:00:00Z","updated_by":"test"}]}
"#,
    );

    let workspace_arg = workspace.path().display().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "plan",
            "review-flow",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(!output.status.success(), "blocked skill should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(env["error"]["details"]["skill"], json!("review-helper"));
}

#[test]
fn workflow_run_is_deferred_without_execution() {
    let root = TestDir::new("workflow-run-deferred");
    let workspace = TestDir::new("workflow-run-workspace");
    write_demo_skills(&root);
    create_review_workflow(&root);
    let workflow_before =
        fs::read_to_string(root.path().join("state/registry/workflows.json")).expect("workflow");
    let workspace_arg = workspace.path().display().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "run",
            "review-flow",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "dry-run run should pass: {env}");
    assert_eq!(env["data"]["status"], json!("deferred"));
    assert_eq!(env["data"]["deferred"], json!(true));
    assert_eq!(env["data"]["hidden"], json!(true));
    assert_eq!(env["data"]["safe_to_run"], json!(false));

    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "run",
            "review-flow",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(!output.status.success(), "non-dry run should be blocked");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(env["error"]["details"]["status"], json!("deferred"));
    assert_eq!(env["error"]["details"]["hidden"], json!(true));
    assert_eq!(env["error"]["details"]["safe_to_run"], json!(false));
    let workflow_after =
        fs::read_to_string(root.path().join("state/registry/workflows.json")).expect("workflow");
    assert_eq!(workflow_after, workflow_before);
}

#[test]
fn workflow_help_hides_run_surface_until_apply_gates_exist() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["workflow", "--help"])
        .output()
        .expect("workflow help");
    assert!(
        output.status.success(),
        "workflow help should pass: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout
            .lines()
            .any(|line| line.trim_start().starts_with("run ")),
        "workflow run should be hidden from public help: {stdout}"
    );
}

#[test]
fn workflow_create_from_skillset_is_preview_only() {
    let root = TestDir::new("workflow-from-skillset");
    write_demo_skills(&root);
    let (output, env) = run_loom(root.path(), &["skillset", "create", "review-pack"]);
    assert!(
        output.status.success(),
        "skillset create should pass: {env}"
    );
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "review-pack", "review-helper"],
    );
    assert!(output.status.success(), "skillset add should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "create",
            "review-preview",
            "--from-skillset",
            "review-pack",
        ],
    );
    assert!(!output.status.success(), "non-dry-run preview should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));

    let (output, env) = run_loom(
        root.path(),
        &[
            "workflow",
            "create",
            "review-preview",
            "--from-skillset",
            "review-pack",
            "--dry-run",
        ],
    );
    assert!(
        output.status.success(),
        "dry-run preview should pass: {env}"
    );
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(
        env["data"]["workflow"]["ordered_node_ids"],
        json!(["review-helper"])
    );
    assert!(!root.path().join("state/registry/workflows.json").exists());
}
