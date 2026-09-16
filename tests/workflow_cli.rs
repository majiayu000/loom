mod common;

use std::{fs, process::Command};

use serde_json::{Value, json};

use common::{TestDir, fake_codex_path, run_loom, run_loom_with_env, write_file, write_skill};

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
fn workflow_create_from_skillset_previews_and_persists_snapshot() {
    let root = TestDir::new("workflow-from-skillset");
    write_demo_skills(&root);
    let (output, env) = run_loom(root.path(), &["skillset", "create", "review-pack"]);
    assert!(output.status.success(), "{env}");
    for skill in ["review-helper", "test-writer"] {
        let (output, env) = run_loom(root.path(), &["skillset", "add", "review-pack", skill]);
        assert!(output.status.success(), "{env}");
    }
    let args = [
        "workflow",
        "create",
        "review-preview",
        "--from-skillset",
        "review-pack",
    ];
    let mut preview_args = args.to_vec();
    preview_args.push("--dry-run");
    let (output, env) = run_loom(root.path(), &preview_args);
    assert!(output.status.success(), "{env}");
    assert_eq!(env["data"]["dry_run"], true);
    let preview = env["data"]["workflow"].clone();
    assert_eq!(
        preview["ordered_node_ids"],
        json!(["review-helper", "test-writer"])
    );
    assert_eq!(preview["external_inputs"], json!(["task"]));
    assert_eq!(
        preview["nodes"][1]["requires"],
        json!(["task", "review-helper_result"])
    );
    assert!(
        preview["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|node| node["mutates_workspace"] == false)
    );
    assert!(!root.path().join("state/registry/workflows.json").exists());
    let (output, env) = run_loom(root.path(), &args);
    assert!(output.status.success(), "{env}");
    assert!(env["data"]["commit"].is_string());
    assert_eq!(env["data"]["workflow"]["nodes"], preview["nodes"]);
    assert_eq!(env["data"]["workflow"]["edges"], preview["edges"]);
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "remove", "review-pack", "test-writer"],
    );
    assert!(output.status.success(), "{env}");
    let (output, env) = run_loom(root.path(), &["workflow", "show", "review-preview"]);
    assert!(output.status.success(), "{env}");
    assert_eq!(
        env["data"]["nodes"], preview["nodes"],
        "persisted workflow must not follow later membership edits"
    );
    let (output, env) = run_loom(root.path(), &args);
    assert!(
        !output.status.success(),
        "duplicate workflow must fail: {env}"
    );
}

#[test]
fn workflow_from_skillset_rejects_missing_empty_and_malformed_sources() {
    let root = TestDir::new("workflow-invalid-skillset");
    let args = [
        "workflow",
        "create",
        "generated",
        "--from-skillset",
        "empty",
    ];
    let (output, env) = run_loom(root.path(), &args);
    assert!(!output.status.success(), "{env}");
    assert_eq!(env["error"]["code"], "SKILL_NOT_FOUND");
    let (output, env) = run_loom(root.path(), &["skillset", "create", "empty"]);
    assert!(output.status.success(), "{env}");
    let (output, env) = run_loom(root.path(), &args);
    assert!(!output.status.success(), "{env}");
    assert_eq!(env["error"]["details"]["validation_code"], "WORKFLOW_EMPTY");
    let path = root.path().join("state/registry/skillsets.json");
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    state["skillsets"][0]["members"] = json!([{"required": true}]);
    write_file(&path, &state.to_string());
    let (output, env) = run_loom(root.path(), &args);
    assert!(!output.status.success(), "{env}");
    assert_eq!(env["error"]["code"], "STATE_CORRUPT");
    assert!(!root.path().join("state/registry/workflows.json").exists());
}

fn workspace_git(workspace: &TestDir, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(workspace.path())
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn executable_workflow() -> (TestDir, TestDir, String, String) {
    let root = TestDir::new("workflow-apply");
    let workspace = TestDir::new("workflow-apply-workspace");
    workspace_git(&workspace, &["init"]);
    workspace_git(&workspace, &["config", "user.name", "Test"]);
    workspace_git(
        &workspace,
        &["config", "user.email", "test@example.invalid"],
    );
    write_file(&workspace.path().join("keep.txt"), "original\n");
    workspace_git(&workspace, &["add", "keep.txt"]);
    workspace_git(&workspace, &["commit", "-m", "initial"]);
    write_demo_skills(&root);
    create_review_workflow(&root);
    for skill in ["review-helper", "test-writer"] {
        let (output, value) = run_loom(
            root.path(),
            &[
                "skill",
                "activate",
                skill,
                "--agent",
                "codex",
                "--scope",
                "project",
                "--workspace",
                workspace.path().to_str().unwrap(),
            ],
        );
        assert!(output.status.success(), "{value}");
    }
    write_file(
        &root.path().join("inputs.json"),
        "{\"task\":\"Write the reviewed result\"}",
    );
    let (output, value) = run_loom(
        root.path(),
        &[
            "workflow",
            "plan",
            "review-flow",
            "--agent",
            "codex",
            "--workspace",
            workspace.path().to_str().unwrap(),
        ],
    );
    assert!(output.status.success(), "{value}");
    let plan = value["data"]["plan_id"].as_str().unwrap().to_string();
    let path = fake_codex_path(
        root.path(),
        r#"#!/bin/sh
printf '%s\n' invoked >> "$LOOM_TEST_WORKFLOW_CALLS"
case "$*" in
  *"node: orient."*)
    printf '%s\n' '{"type":"agent_message","content":"{\"plan\":\"Write result.txt\"}"}'
    ;;
  *"node: test."*)
    printf '%s\n' 'reviewed result' > result.txt
    if [ "$LOOM_TEST_WORKFLOW_FAIL" = 1 ]; then exit 1; fi
    printf '%s\n' '{"type":"agent_message","content":"{\"tests\":\"result verified\"}"}'
    ;;
  *) exit 23 ;;
esac
"#,
    );
    (root, workspace, plan, path)
}

fn apply_workflow(
    root: &TestDir,
    path: &str,
    plan: &str,
    approve: bool,
    dry_run: bool,
    fail: bool,
) -> (std::process::Output, Value) {
    let inputs = root.path().join("inputs.json");
    let calls = root.path().join("calls.txt");
    let mut args = vec![
        "workflow",
        "apply",
        plan,
        "--idempotency-key",
        "reviewed-run",
        "--inputs",
        inputs.to_str().unwrap(),
    ];
    if approve {
        args.extend(["--approve", "approve-test"]);
    }
    if dry_run {
        args.push("--dry-run");
    }
    run_loom_with_env(
        root.path(),
        &[
            ("PATH", path),
            ("LOOM_TEST_WORKFLOW_CALLS", calls.to_str().unwrap()),
            ("LOOM_TEST_WORKFLOW_FAIL", if fail { "1" } else { "0" }),
        ],
        &args,
    )
}

#[test]
fn workflow_apply_requires_approval_and_replays_without_rerunning_nodes() {
    let (root, workspace, plan, path) = executable_workflow();
    let (output, value) = apply_workflow(&root, &path, &plan, false, true, false);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["ready"], false);
    assert!(!root.path().join("calls.txt").exists());
    let (output, value) = apply_workflow(&root, &path, &plan, false, false, false);
    assert!(!output.status.success(), "{value}");
    assert!(!root.path().join("calls.txt").exists());
    write_file(&workspace.path().join("keep.txt"), "user staged edit\n");
    workspace_git(&workspace, &["add", "keep.txt"]);
    let index = workspace_git(&workspace, &["write-tree"]);
    let (output, value) = apply_workflow(&root, &path, &plan, true, false, false);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["execution"]["status"], "completed");
    assert_eq!(
        fs::read_to_string(workspace.path().join("result.txt")).unwrap(),
        "reviewed result\n"
    );
    assert_eq!(workspace_git(&workspace, &["write-tree"]), index);
    let checkpoint = value["data"]["execution"]["nodes"][1]["checkpoint_ref"]
        .as_str()
        .unwrap();
    assert_eq!(
        workspace_git(&workspace, &["show", &format!("{checkpoint}:keep.txt")]),
        "user staged edit"
    );
    let (output, value) = apply_workflow(&root, &path, &plan, true, false, false);
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["replayed"], true);
    assert_eq!(
        fs::read_to_string(root.path().join("calls.txt"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    write_file(
        &root.path().join("inputs.json"),
        "{\"task\":\"different input\"}",
    );
    let (output, _) = apply_workflow(&root, &path, &plan, true, false, false);
    assert!(!output.status.success());
}

#[test]
fn failed_workflow_retains_checkpoint_and_refuses_automatic_retry() {
    let (root, workspace, plan, path) = executable_workflow();
    let (output, value) = apply_workflow(&root, &path, &plan, true, false, true);
    assert!(!output.status.success(), "{value}");
    assert_eq!(value["error"]["details"]["execution"]["status"], "failed");
    assert!(value["error"]["details"]["execution"]["nodes"][1]["checkpoint_ref"].is_string());
    assert!(workspace.path().join("result.txt").exists());
    let (output, _) = apply_workflow(&root, &path, &plan, true, false, false);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(root.path().join("calls.txt"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn workflow_apply_rejects_stale_sources_before_starting_codex() {
    let (root, _workspace, plan, path) = executable_workflow();
    write_file(
        &root.path().join("skills/review-helper/SKILL.md"),
        "changed source\n",
    );
    let (output, value) = apply_workflow(&root, &path, &plan, true, false, false);
    assert!(!output.status.success(), "{value}");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("stale")
    );
    assert!(!root.path().join("calls.txt").exists());
}

#[test]
fn workflow_apply_checks_required_dependencies_before_starting_codex() {
    let (root, workspace, _plan, path) = executable_workflow();
    write_file(
        &root.path().join("skills/review-helper/loom.skill.toml"),
        "requires_tools = [\"loom-test-deliberately-unavailable-tool\"]\n",
    );
    let (output, value) = run_loom(
        root.path(),
        &[
            "workflow",
            "plan",
            "review-flow",
            "--agent",
            "codex",
            "--workspace",
            workspace.path().to_str().unwrap(),
        ],
    );
    assert!(output.status.success(), "{value}");
    let plan = value["data"]["plan_id"].as_str().unwrap();
    let (output, value) = apply_workflow(&root, &path, plan, true, false, false);
    assert!(!output.status.success(), "{value}");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("dependencies")
    );
    assert!(!root.path().join("calls.txt").exists());
}
