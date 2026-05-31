use std::fs;
use std::path::Path;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, target_add};
use common::{TestDir, run_loom, write_skill};

fn write_example_skill(root: &Path, skill: &str) {
    write_skill(root, skill, &format!("# {}\n\nexample skill\n", skill));
}

fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn operations_log(root: &Path) -> String {
    read_text(&root.join("state/registry/ops/operations.jsonl"))
}

fn pending_log(root: &Path) -> String {
    read_text(&root.join("state/pending_ops.jsonl"))
}

fn setup_binding(root: &Path, ownership: &str, workspace: &Path) -> String {
    setup_binding_with_target(root, ownership, workspace).0
}

fn setup_binding_with_target(root: &Path, ownership: &str, workspace: &Path) -> (String, String) {
    write_example_skill(root, "model-onboarding");
    let (save_output, _) = save_skill(root, "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.join("live/claude-project-a");
    fs::create_dir_all(&target_path).expect("target path");
    let (target_output, target_env) = target_add(root, "claude", &target_path, ownership);
    assert!(target_output.status.success(), "target add should succeed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    let workspace = workspace.to_string_lossy().into_owned();
    let (binding_output, binding_env) = binding_add(
        root,
        "claude",
        "default",
        "path-prefix",
        &workspace,
        target_id,
    );
    assert!(
        binding_output.status.success(),
        "binding add failed: stderr={} stdout={}",
        String::from_utf8_lossy(&binding_output.stderr),
        String::from_utf8_lossy(&binding_output.stdout)
    );
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id")
        .to_string();
    (binding_id, target_id.to_string())
}

#[test]
fn agent_preflight_resolves_selectors_without_registry_operation() {
    let root = TestDir::new("agent-preflight-ready");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    let binding_id = setup_binding(root.path(), "managed", &workspace);
    let operations_before = operations_log(root.path());

    let workspace_arg = workspace.to_string_lossy().into_owned();
    let (output, env) = run_loom(
        root.path(),
        &[
            "agent",
            "preflight",
            "--agent",
            "claude",
            "--workspace",
            &workspace_arg,
            "--skill",
            "model-onboarding",
        ],
    );

    assert!(output.status.success(), "preflight should succeed");
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["safe_to_run"], Value::Bool(true));
    assert_eq!(
        env["data"]["required_selectors"]["binding_id"],
        Value::String(binding_id)
    );
    assert!(
        env["data"]["next_commands"][0]
            .as_str()
            .expect("next command")
            .contains("skill project model-onboarding")
    );
    assert_eq!(operations_log(root.path()), operations_before);
}

#[test]
fn agent_preflight_uses_rule_target_for_skill_selector() {
    let root = TestDir::new("agent-preflight-rule-target");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    let binding_id = setup_binding(root.path(), "managed", &workspace);

    let alternate_target_path = root.path().join("live/claude-project-b");
    let (target_output, target_env) =
        target_add(root.path(), "claude", &alternate_target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let alternate_target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    let (project_output, _) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &binding_id,
            "--target",
            alternate_target_id,
            "--method",
            "copy",
        ],
    );
    assert!(project_output.status.success(), "project should succeed");

    let workspace_arg = workspace.to_string_lossy().into_owned();
    let (output, env) = run_loom(
        root.path(),
        &[
            "agent",
            "preflight",
            "--agent",
            "claude",
            "--workspace",
            &workspace_arg,
            "--skill",
            "model-onboarding",
        ],
    );

    assert!(output.status.success(), "preflight should succeed");
    assert_eq!(
        env["data"]["required_selectors"]["target_id"],
        Value::String(alternate_target_id.to_string())
    );
    assert_eq!(
        env["data"]["target_paths"][0],
        Value::String(
            fs::canonicalize(&alternate_target_path)
                .expect("canonical target path")
                .display()
                .to_string()
        )
    );
    let next_command = env["data"]["next_commands"][0]
        .as_str()
        .expect("next command");
    assert!(
        next_command.contains(&format!("--target {alternate_target_id}")),
        "next command must preserve rule target: {next_command}"
    );
}

#[test]
fn agent_preflight_blocks_ambiguous_skill_rules_before_selecting_target() {
    let root = TestDir::new("agent-preflight-ambiguous-rule-target");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    let (binding_id, default_target_id) =
        setup_binding_with_target(root.path(), "managed", &workspace);

    let alternate_target_path = root.path().join("live/claude-project-b");
    let (target_output, target_env) =
        target_add(root.path(), "claude", &alternate_target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let alternate_target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    for target_id in [default_target_id.as_str(), alternate_target_id] {
        let (project_output, _) = run_loom(
            root.path(),
            &[
                "skill",
                "project",
                "model-onboarding",
                "--binding",
                &binding_id,
                "--target",
                target_id,
                "--method",
                "copy",
            ],
        );
        assert!(project_output.status.success(), "project should succeed");
    }

    let workspace_arg = workspace.to_string_lossy().into_owned();
    let (output, env) = run_loom(
        root.path(),
        &[
            "agent",
            "preflight",
            "--agent",
            "claude",
            "--workspace",
            &workspace_arg,
            "--skill",
            "model-onboarding",
        ],
    );

    assert!(output.status.success(), "preflight should return a plan");
    assert_eq!(env["data"]["safe_to_run"], Value::Bool(false));
    assert_eq!(
        env["data"]["risks"][0]["code"],
        Value::String("AMBIGUOUS_SKILL_RULE".to_string())
    );
    assert_eq!(env["data"]["required_selectors"]["target_id"], Value::Null);
    assert_eq!(env["data"]["target_paths"], Value::Array(Vec::new()));
    assert_eq!(env["data"]["next_commands"], Value::Array(Vec::new()));
}

#[test]
fn agent_preflight_reports_ambiguous_workspace_binding() {
    let root = TestDir::new("agent-preflight-ambiguous");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    setup_binding(root.path(), "managed", &workspace);

    let second_target = root.path().join("live/claude-project-b");
    let (target_output, target_env) = target_add(root.path(), "claude", &second_target, "managed");
    assert!(
        target_output.status.success(),
        "second target add should succeed"
    );
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace_arg = workspace.to_string_lossy().into_owned();
    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "second",
        "path-prefix",
        &workspace_arg,
        target_id,
    );
    assert!(
        binding_output.status.success(),
        "second binding add should succeed"
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "agent",
            "preflight",
            "--agent",
            "claude",
            "--workspace",
            &workspace_arg,
            "--skill",
            "model-onboarding",
        ],
    );

    assert!(output.status.success(), "preflight should return a plan");
    assert_eq!(env["data"]["safe_to_run"], Value::Bool(false));
    assert_eq!(
        env["data"]["risks"][0]["code"],
        Value::String("AMBIGUOUS_BINDING".to_string())
    );
}

#[test]
fn project_dry_run_preserves_explicit_target_in_next_command() {
    let root = TestDir::new("project-dry-run-explicit-target");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    let binding_id = setup_binding(root.path(), "managed", &workspace);
    let alternate_target_path = root.path().join("live/claude-project-b");
    let (target_output, target_env) =
        target_add(root.path(), "claude", &alternate_target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let alternate_target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &binding_id,
            "--target",
            alternate_target_id,
            "--dry-run",
        ],
    );

    assert!(output.status.success(), "dry-run should succeed");
    let next_command = env["data"]["next_commands"][0]
        .as_str()
        .expect("next command");
    assert!(
        next_command.contains(&format!("--target {alternate_target_id}")),
        "next command must preserve explicit target: {next_command}"
    );
}

#[test]
fn project_dry_run_reports_plan_without_touching_state_or_target() {
    let root = TestDir::new("project-dry-run");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    let binding_id = setup_binding(root.path(), "managed", &workspace);
    let target_skill_path = root.path().join("live/claude-project-a/model-onboarding");
    let operations_before = operations_log(root.path());
    let pending_before = pending_log(root.path());

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &binding_id,
            "--dry-run",
        ],
    );

    assert!(output.status.success(), "dry-run should succeed");
    assert_eq!(env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(
        env["data"]["operation"],
        Value::String("skill.project".to_string())
    );
    assert_eq!(env["data"]["safe_to_run"], Value::Bool(true));
    assert!(
        !target_skill_path.exists(),
        "dry-run must not materialize projection"
    );
    assert_eq!(operations_log(root.path()), operations_before);
    assert_eq!(pending_log(root.path()), pending_before);
}

#[test]
fn project_dry_run_reports_unsafe_observed_target() {
    let root = TestDir::new("project-dry-run-observed");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    let binding_id = setup_binding(root.path(), "observed", &workspace);

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &binding_id,
            "--dry-run",
        ],
    );

    assert!(
        output.status.success(),
        "dry-run should return a blocked plan"
    );
    assert_eq!(env["data"]["safe_to_run"], Value::Bool(false));
    assert!(
        env["data"]["risks"]
            .as_array()
            .expect("risks")
            .iter()
            .any(|risk| risk["code"] == "TARGET_NOT_MANAGED")
    );
}

#[test]
fn sync_push_dry_run_does_not_clear_pending_queue() {
    let root = TestDir::new("sync-push-dry-run");
    let workspace = root.path().join("work/project-a");
    fs::create_dir_all(&workspace).expect("workspace");
    setup_binding(root.path(), "managed", &workspace);
    let pending_before = pending_log(root.path());

    let (output, env) = run_loom(root.path(), &["sync", "push", "--dry-run"]);

    assert!(
        output.status.success(),
        "dry-run should return a blocked plan"
    );
    assert_eq!(
        env["data"]["operation"],
        Value::String("sync.push".to_string())
    );
    assert_eq!(env["data"]["safe_to_run"], Value::Bool(false));
    assert!(
        env["data"]["risks"]
            .as_array()
            .expect("risks")
            .iter()
            .any(|risk| risk["code"] == "REMOTE_NOT_CONFIGURED")
    );
    assert_eq!(pending_log(root.path()), pending_before);
}
