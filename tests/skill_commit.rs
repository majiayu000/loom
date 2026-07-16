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
    project_copy_named(root, skill, "project-a").0
}

fn project_copy_named(root: &TestDir, skill: &str, suffix: &str) -> (std::path::PathBuf, String) {
    let target_path = root.path().join(format!("live/claude-{suffix}"));
    let (target_output, target_env) = target_add(root.path(), "claude", &target_path, "managed");
    assert_success(&target_output, "target add");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let (binding_output, binding_env) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        &format!("/tmp/{suffix}"),
        target_id,
    );
    assert_success(&binding_output, "binding add");
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (project_output, project_env) = skill_project(root.path(), skill, binding_id, Some("copy"));
    assert_success(&project_output, "skill project");
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("instance id")
        .to_string();
    (target_path.join(skill), instance_id)
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
    let actions = env["error"]["next_actions"]
        .as_array()
        .expect("ambiguous commit next_actions");
    assert_eq!(actions.len(), 2);
    assert!(actions.iter().all(|action| {
        action["cmd"]
            .as_str()
            .is_some_and(|cmd| cmd.starts_with("loom ") && cmd.contains("--json"))
    }));
}

#[test]
fn skill_commit_ambiguous_actions_select_each_dirty_projection() {
    let root = TestDir::new("skill-commit-ambiguous-multiple-projections");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
    );
    assert_success(
        &save_skill(root.path(), "model-onboarding").0,
        "initial source commit",
    );
    let (first_live_dir, first_instance) =
        project_copy_named(&root, "model-onboarding", "project-a");
    let (second_live_dir, second_instance) =
        project_copy_named(&root, "model-onboarding", "project-b");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
    );
    fs::write(
        first_live_dir.join("SKILL.md"),
        "# model-onboarding\n\nprojection a v2\n",
    )
    .expect("edit first projection");
    fs::write(
        second_live_dir.join("SKILL.md"),
        "# model-onboarding\n\nprojection b v2\n",
    )
    .expect("edit second projection");

    let (output, env) = run_loom(root.path(), &["skill", "commit", "model-onboarding"]);

    assert!(!output.status.success(), "ambiguous commit should fail");
    assert_eq!(
        env["error"]["code"],
        Value::String("COMMIT_DIRECTION_AMBIGUOUS".to_string())
    );
    let actions = env["error"]["next_actions"]
        .as_array()
        .expect("ambiguous commit next_actions");
    assert_eq!(actions.len(), 3);
    for instance in [&first_instance, &second_instance] {
        let expected = format!(
            "loom skill commit model-onboarding --from-projection --instance {instance} --json"
        );
        assert!(
            actions.iter().any(|action| action["cmd"] == expected),
            "missing directly runnable action for {instance}: {actions:?}"
        );
    }

    let (selected_output, selected_env) = run_loom(
        root.path(),
        &[
            "skill",
            "commit",
            "model-onboarding",
            "--from-projection",
            "--instance",
            &first_instance,
        ],
    );
    assert!(
        !selected_output.status.success(),
        "source drift should still protect the selected projection capture"
    );
    assert_eq!(
        selected_env["error"]["code"],
        Value::String("CAPTURE_CONFLICT".to_string()),
        "the action must select the projection instead of failing for a missing selector"
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
