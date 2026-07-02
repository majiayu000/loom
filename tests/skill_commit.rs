mod common;

use std::fs;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};
use serde_json::Value;

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn project_copy(root: &TestDir, skill: &str) -> std::path::PathBuf {
    let target_path = root.path().join("live/claude-project-a");
    assert_success(
        &target_add(root.path(), "claude", &target_path, "managed").0,
        "target add",
    );
    assert_success(
        &binding_add(
            root.path(),
            "claude",
            "default",
            "path-prefix",
            "/tmp/project-a",
            "target_claude_claude_project_a",
        )
        .0,
        "binding add",
    );
    assert_success(
        &skill_project(root.path(), skill, "bind_claude_project_a", Some("copy")).0,
        "skill project",
    );
    target_path.join(skill)
}

#[test]
fn skill_commit_auto_detects_source_dirty_only() {
    let root = TestDir::new("skill-commit-source");
    write_skill(root.path(), "demo", "# demo\n\nsource v1\n");

    let (output, env) = run_loom(root.path(), &["skill", "commit", "demo"]);

    assert_success(&output, "skill commit");
    assert_eq!(
        env["data"]["direction"],
        Value::String("source".to_string())
    );
    assert!(
        env["data"]["commit"].is_string(),
        "commit should be recorded"
    );
}

#[test]
fn skill_commit_auto_detects_projection_dirty_only() {
    let root = TestDir::new("skill-commit-projection");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
    );
    assert_success(
        &save_skill(root.path(), "model-onboarding").0,
        "initial source commit",
    );
    let live_dir = project_copy(&root, "model-onboarding");
    fs::write(
        live_dir.join("SKILL.md"),
        "# model-onboarding\n\nprojection v2\n",
    )
    .expect("edit projection");

    let (output, env) = run_loom(root.path(), &["skill", "commit", "model-onboarding"]);

    assert_success(&output, "skill commit");
    assert_eq!(
        env["data"]["direction"],
        Value::String("projection".to_string())
    );
    assert_eq!(env["data"]["capture"]["noop"], Value::Bool(false));
    let source = fs::read_to_string(root.path().join("skills/model-onboarding/SKILL.md"))
        .expect("read source skill");
    assert!(source.contains("projection v2"));
}

#[test]
fn skill_commit_reports_ambiguous_when_source_and_projection_are_dirty() {
    let root = TestDir::new("skill-commit-ambiguous");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
    );
    assert_success(
        &save_skill(root.path(), "model-onboarding").0,
        "initial source commit",
    );
    let live_dir = project_copy(&root, "model-onboarding");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
    );
    fs::write(
        live_dir.join("SKILL.md"),
        "# model-onboarding\n\nprojection v2\n",
    )
    .expect("edit projection");

    let (output, env) = run_loom(root.path(), &["skill", "commit", "model-onboarding"]);

    assert!(
        !output.status.success(),
        "ambiguous commit should fail: {env:?}"
    );
    assert_eq!(
        env["error"]["code"],
        Value::String("COMMIT_DIRECTION_AMBIGUOUS".to_string())
    );
    assert_eq!(env["error"]["details"]["source_dirty"], Value::Bool(true));
    assert_eq!(
        env["error"]["details"]["projection_dirty"],
        Value::Bool(true)
    );
}

#[test]
fn skill_commit_noops_when_neither_side_is_dirty() {
    let root = TestDir::new("skill-commit-noop");
    write_skill(root.path(), "demo", "# demo\n\nsource v1\n");
    assert_success(&save_skill(root.path(), "demo").0, "initial source commit");

    let (output, env) = run_loom(root.path(), &["skill", "commit", "demo"]);

    assert_success(&output, "skill commit noop");
    assert_eq!(env["data"]["noop"], Value::Bool(true));
    assert_eq!(env["data"]["source_dirty"], Value::Bool(false));
    assert_eq!(env["data"]["projection_dirty"], Value::Bool(false));
}
