mod common;

use std::fs;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, run_loom_with_env, write_skill};
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

#[test]
fn workspace_doctor_marks_pending_queue_warnings_unhealthy() {
    let root = TestDir::new("doctor-pending-warning");
    let target_path = root.path().join("live/claude-project-a");
    assert!(
        target_add(root.path(), "claude", &target_path, "managed")
            .0
            .status
            .success()
    );
    fs::write(root.path().join("state/pending_ops.jsonl"), "not-json\n")
        .expect("write malformed pending queue");

    let (output, env) = run_loom(root.path(), &["workspace", "doctor"]);

    assert!(output.status.success(), "doctor should succeed");
    assert_eq!(env["data"]["healthy"], Value::Bool(false));
    assert_eq!(
        env["data"]["checks"]["pending_queue"]["warnings"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let check = find_check(&env, "pending_queue_warnings");
    assert_eq!(check["ok"], Value::Bool(false));
    assert_eq!(check["severity"], Value::String("warning".to_string()));
    assert_eq!(check["details"]["warning_count"], Value::from(1));
}

#[test]
fn workspace_doctor_reports_agent_skill_inventory() {
    let root = TestDir::new("doctor-agent-inventory");
    let fake_home = root.path().join("fake-home");
    fs::create_dir_all(fake_home.join(".claude/skills")).expect("fake claude skill dir");
    fs::create_dir_all(fake_home.join(".codex/skills")).expect("fake codex skill dir");

    let target_path = root.path().join("live/claude-project-a");
    assert!(
        target_add(root.path(), "claude", &target_path, "managed")
            .0
            .status
            .success()
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", fake_home.to_str().expect("HOME utf-8"))],
        &["workspace", "doctor"],
    );
    assert!(output.status.success(), "doctor should succeed");
    assert_eq!(env["ok"], Value::Bool(true));

    let inventory = find_check(&env, "agent_skill_inventory");
    assert_eq!(inventory["section"], Value::String("agents".to_string()));
    assert_eq!(inventory["ok"], Value::Bool(true));
    assert_eq!(inventory["severity"], Value::String("info".to_string()));
    assert_eq!(inventory["details"]["home_set"], Value::Bool(true));
    assert_eq!(inventory["details"]["total"], Value::from(10));

    let agents = inventory["details"]["agents"]
        .as_array()
        .expect("agents array");
    assert_eq!(agents.len(), 10, "all known agents must be reported");

    let claude = agents
        .iter()
        .find(|a| a["agent"] == "claude")
        .expect("claude entry");
    assert_eq!(claude["present"], Value::Bool(true));
    assert_eq!(claude["registered_target_count"], Value::from(0));

    let codex = agents
        .iter()
        .find(|a| a["agent"] == "codex")
        .expect("codex entry");
    assert_eq!(codex["present"], Value::Bool(true));

    let cursor = agents
        .iter()
        .find(|a| a["agent"] == "cursor")
        .expect("cursor entry");
    assert_eq!(cursor["present"], Value::Bool(false));

    let legacy = &env["data"]["checks"]["agent_skill_dirs"]["agents"];
    assert_eq!(
        legacy.as_array().map(Vec::len),
        Some(10),
        "checks.agent_skill_dirs must mirror inventory"
    );
}

#[test]
fn workspace_doctor_agent_skill_inventory_reports_registered_targets() {
    let root = TestDir::new("doctor-agent-inventory-registered-target");
    let fake_home = root.path().join("fake-home");
    let claude_skills = fake_home.join(".claude/skills");
    fs::create_dir_all(&claude_skills).expect("fake claude skill dir");

    assert!(
        target_add(root.path(), "claude", &claude_skills, "observed")
            .0
            .status
            .success()
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", fake_home.to_str().expect("HOME utf-8"))],
        &["workspace", "doctor"],
    );
    assert!(output.status.success(), "doctor should succeed");

    let inventory = find_check(&env, "agent_skill_inventory");
    let agents = inventory["details"]["agents"]
        .as_array()
        .expect("agents array");
    let claude = agents
        .iter()
        .find(|a| a["agent"] == "claude")
        .expect("claude entry");
    assert_eq!(claude["present"], Value::Bool(true));
    assert_eq!(claude["registered_target_count"], Value::from(1));
    assert_eq!(
        claude["registered_targets"][0]["ownership"],
        Value::String("observed".to_string())
    );
    assert_eq!(
        claude["registered_targets"][0]["target_id"],
        Value::String("target_claude_claude_skills".to_string())
    );
}

#[test]
fn workspace_doctor_agent_skill_inventory_when_home_unset() {
    let root = TestDir::new("doctor-agent-inventory-no-home");
    let target_path = root.path().join("live/claude-project-a");
    assert!(
        target_add(root.path(), "claude", &target_path, "managed")
            .0
            .status
            .success()
    );

    let (output, env) = run_loom_with_env(root.path(), &[("HOME", "")], &["workspace", "doctor"]);
    assert!(
        output.status.success(),
        "doctor should still succeed without HOME"
    );

    let inventory = find_check(&env, "agent_skill_inventory");
    assert_eq!(inventory["ok"], Value::Bool(true));
    assert_eq!(inventory["details"]["home_set"], Value::Bool(false));
    assert_eq!(inventory["details"]["total"], Value::from(0));
}
