use std::fs;
use std::path::Path;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, run_loom_with_env, write_skill};

fn write_example_skill(root: &std::path::Path, skill: &str) {
    write_skill(root, skill, &format!("# {}\n\nexample skill\n", skill));
}

#[test]
fn skill_project_creates_projection_rule_and_instance() {
    let root = TestDir::new("v3-skill-project");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_claude_project_a",
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        None,
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

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/observed-claude");
    fs::create_dir_all(&target_path).expect("create observed target path");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "observed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_observed_claude",
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        None,
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

#[test]
fn skill_project_backs_up_existing_projection_path_before_replace() {
    let root = TestDir::new("v3-skill-project-backup");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_claude_project_a",
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let existing_projection = target_path.join("model-onboarding");
    fs::create_dir_all(&existing_projection).expect("create existing projection path");
    fs::write(
        existing_projection.join("legacy.txt"),
        "legacy projection content",
    )
    .expect("write legacy projection marker");

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        Some("copy"),
    );
    assert!(
        project_output.status.success(),
        "project failed: stderr={} stdout={}",
        String::from_utf8_lossy(&project_output.stderr),
        String::from_utf8_lossy(&project_output.stdout)
    );

    let backup_path = project_env["data"]["backup"]["backup_path"]
        .as_str()
        .expect("backup path should be returned");
    let backup_path = Path::new(backup_path);
    assert!(backup_path.exists(), "backup path should exist");
    assert!(
        backup_path.join("legacy.txt").exists(),
        "backup should preserve replaced content"
    );
}

#[test]
fn skill_project_rolls_back_projection_after_post_materialize_failure() {
    let root = TestDir::new("v3-skill-project-rollback");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    assert!(
        target_add(root.path(), "claude", &target_path, "managed")
            .0
            .status
            .success()
    );
    assert!(
        binding_add(
            root.path(),
            "claude",
            "default",
            "path-prefix",
            "/tmp/project-a",
            "target_claude_claude_project_a",
        )
        .0
        .status
        .success()
    );

    let existing_projection = target_path.join("model-onboarding");
    fs::create_dir_all(&existing_projection).expect("create existing projection path");
    fs::write(
        existing_projection.join("legacy.txt"),
        "legacy projection content",
    )
    .expect("write legacy projection marker");

    let (project_output, project_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_project_after_materialize")],
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
            "--method",
            "copy",
        ],
    );

    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert!(
        existing_projection.join("legacy.txt").exists(),
        "legacy projection should be restored after failure"
    );
    assert!(
        !existing_projection.join("SKILL.md").exists(),
        "failed projection should not leave copied skill files"
    );

    let rules = fs::read_to_string(root.path().join("state/v3/rules.json")).expect("read rules");
    let projections = fs::read_to_string(root.path().join("state/v3/projections.json"))
        .expect("read projections");
    assert!(
        !rules.contains("model-onboarding"),
        "rules state should roll back"
    );
    assert!(
        !projections.contains("model-onboarding"),
        "projection state should roll back"
    );
}
