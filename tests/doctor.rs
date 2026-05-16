mod common;

use std::fs;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};
use serde_json::Value;

fn find_check<'a>(env: &'a Value, id_prefix: &str) -> &'a Value {
    env["data"]["checks_v1"]
        .as_array()
        .expect("checks_v1 array")
        .iter()
        .find(|check| {
            check["id"]
                .as_str()
                .map(|id| id.starts_with(id_prefix))
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("missing doctor check with prefix {id_prefix}"))
}

#[test]
fn workspace_doctor_reports_missing_target_path_with_next_action() {
    let root = TestDir::new("doctor-missing-target-path");
    let target_path = root.path().join("live/claude-project-a");
    assert!(
        target_add(root.path(), "claude", &target_path, "managed")
            .0
            .status
            .success()
    );
    fs::remove_dir_all(&target_path).expect("remove target path");

    let (output, env) = run_loom(root.path(), &["workspace", "doctor"]);

    assert!(
        output.status.success(),
        "doctor failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["healthy"], Value::Bool(false));
    let check = find_check(&env, "target_path_exists:target_claude_claude_project_a");
    assert_eq!(check["ok"], Value::Bool(false));
    assert_eq!(check["severity"], Value::String("error".to_string()));
    assert_eq!(
        check["next_action"],
        Value::String("recreate the target path or remove/update the target".to_string())
    );
}

#[test]
fn workspace_doctor_reports_binding_target_agent_mismatch() {
    let root = TestDir::new("doctor-binding-agent-mismatch");
    let target_path = root.path().join("live/codex-project-a");
    let (target_output, target_env) = target_add(root.path(), "codex", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    assert!(
        binding_add(
            root.path(),
            "claude",
            "default",
            "path-prefix",
            "/tmp/project-a",
            target_id,
        )
        .0
        .status
        .success()
    );

    let (output, env) = run_loom(root.path(), &["workspace", "doctor"]);

    assert!(output.status.success(), "doctor should succeed");
    assert_eq!(env["data"]["healthy"], Value::Bool(false));
    let check = find_check(&env, "binding_target_agent_match:bind_claude_project_a");
    assert_eq!(check["ok"], Value::Bool(false));
    assert_eq!(
        check["details"]["binding_agent"],
        Value::String("claude".to_string())
    );
    assert_eq!(
        check["details"]["target_agent"],
        Value::String("codex".to_string())
    );
}

#[test]
fn workspace_doctor_reports_missing_projection_path() {
    let root = TestDir::new("doctor-missing-projection-path");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );
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
    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        Some("copy"),
    );
    assert!(project_output.status.success(), "project should succeed");
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("projection instance id")
        .to_string();
    fs::remove_dir_all(target_path.join("model-onboarding")).expect("remove projection path");

    let (output, env) = run_loom(root.path(), &["workspace", "doctor"]);

    assert!(output.status.success(), "doctor should succeed");
    assert_eq!(env["data"]["healthy"], Value::Bool(false));
    let check = find_check(&env, &format!("projection_path_exists:{instance_id}"));
    assert_eq!(check["ok"], Value::Bool(false));
    assert_eq!(
        check["next_action"],
        Value::String("rerun loom skill project or clean the orphaned projection".to_string())
    );
}
