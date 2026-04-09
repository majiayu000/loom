use std::fs;

use serde_json::Value;

mod common;

use common::{TestDir, run_loom, write_skill};

fn write_example_skill(root: &std::path::Path, skill: &str) {
    write_skill(root, skill, &format!("# {}\n\nexample skill\n", skill));
}

#[test]
fn skill_project_creates_projection_rule_and_instance() {
    let root = TestDir::new("v3-skill-project");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = run_loom(root.path(), &["skill", "save", "model-onboarding"]);
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    let target_path_str = target_path.to_string_lossy().to_string();
    let (target_output, _) = run_loom(
        root.path(),
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            &target_path_str,
            "--ownership",
            "managed",
        ],
    );
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = run_loom(
        root.path(),
        &[
            "workspace",
            "binding",
            "add",
            "--agent",
            "claude",
            "--profile",
            "default",
            "--matcher-kind",
            "path-prefix",
            "--matcher-value",
            "/tmp/project-a",
            "--target",
            "target_claude_claude_project_a",
        ],
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
        ],
    );
    assert!(
        project_output.status.success(),
        "project failed: stderr={} stdout={}",
        String::from_utf8_lossy(&project_output.stderr),
        String::from_utf8_lossy(&project_output.stdout)
    );
    assert_eq!(project_env["ok"], Value::Bool(true));
    assert_eq!(
        project_env["data"]["projection"]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
    assert_eq!(
        project_env["data"]["projection"]["target_id"],
        Value::String("target_claude_claude_project_a".to_string())
    );
    assert_eq!(
        project_env["data"]["projection"]["method"],
        Value::String("symlink".to_string())
    );
    assert_eq!(
        project_env["meta"]["op_id"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );

    let projected_path = target_path.join("model-onboarding");
    assert!(projected_path.exists(), "projected path should exist");

    let (binding_show_output, binding_show_env) = run_loom(
        root.path(),
        &["workspace", "binding", "show", "bind_claude_project_a"],
    );
    assert!(
        binding_show_output.status.success(),
        "binding show should succeed"
    );
    assert_eq!(
        binding_show_env["data"]["rules"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        binding_show_env["data"]["projections"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn skill_project_rejects_unmanaged_target_ownership() {
    let root = TestDir::new("v3-skill-project-observed");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = run_loom(root.path(), &["skill", "save", "model-onboarding"]);
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/observed-claude");
    fs::create_dir_all(&target_path).expect("create observed target path");
    let target_path_str = target_path.to_string_lossy().to_string();
    let (target_output, _) = run_loom(
        root.path(),
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            &target_path_str,
            "--ownership",
            "observed",
        ],
    );
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = run_loom(
        root.path(),
        &[
            "workspace",
            "binding",
            "add",
            "--agent",
            "claude",
            "--profile",
            "default",
            "--matcher-kind",
            "path-prefix",
            "--matcher-value",
            "/tmp/project-a",
            "--target",
            "target_claude_observed_claude",
        ],
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
        ],
    );
    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert_eq!(
        project_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}
