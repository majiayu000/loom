use std::path::Path;

use serde_json::Value;

mod common;

use common::{TestDir, run_loom_with_env, write_minimal_registry_state};

fn run_loom_ok(root: &Path, args: &[&str]) -> Value {
    let (output, env) = run_loom_with_env(root, &[], args);
    assert!(
        output.status.success(),
        "loom failed: status={:?} stderr={} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    env
}

#[test]
fn ops_list_activity_returns_merged_read_model_rows() {
    let root = TestDir::new("ops-list-activity");
    write_minimal_registry_state(root.path(), 1);

    let env = run_loom_ok(root.path(), &["ops", "list", "--activity"]);

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["cmd"], "ops.list");
    let data = &env["data"];
    assert_eq!(data["state_model"], "registry");
    assert_eq!(data["count"], 1);
    assert_eq!(data["registry_count"], 1);
    assert_eq!(data["audit_count"], 0);
    assert_eq!(data["loaded_count"], 1);
    assert_eq!(data["offset"], 0);
    assert_eq!(data["has_more"], Value::Bool(false));
    assert!(data["checkpoint"].is_object(), "{env}");

    let op = &data["operations"][0];
    assert_eq!(op["op_id"], "op_001");
    assert_eq!(op["audit_id"], Value::Null);
    assert_eq!(op["source"], "registry");
    assert_eq!(op["intent"], "skill.project");
    assert_eq!(op["status"], "succeeded");
    assert_eq!(op["skill"], "model-onboarding");
    assert_eq!(op["binding"], "bind_claude_project_a");
    assert!(
        op.get("payload").is_none() && op.get("effects").is_none(),
        "activity rows must not expose raw payload/effects: {env}"
    );
}

#[test]
fn ops_list_activity_honors_limit_and_offset() {
    let root = TestDir::new("ops-list-activity-page");
    write_minimal_registry_state(root.path(), 1);

    let env = run_loom_ok(
        root.path(),
        &["ops", "list", "--activity", "--limit", "1", "--offset", "1"],
    );

    let data = &env["data"];
    assert_eq!(data["count"], 1);
    assert_eq!(data["loaded_count"], 0);
    assert_eq!(data["offset"], 1);
    assert_eq!(data["limit"], 1);
    assert_eq!(data["has_more"], Value::Bool(false));
    assert_eq!(data["operations"], serde_json::json!([]));
}

#[test]
fn ops_list_activity_fails_when_registry_state_is_missing() {
    let root = TestDir::new("ops-list-activity-missing");

    let (output, env) = run_loom_with_env(root.path(), &[], &["ops", "list", "--activity"]);

    assert!(
        !output.status.success(),
        "ops list --activity unexpectedly succeeded: {env}"
    );
    assert_eq!(env["error"]["code"], "STATE_NOT_INITIALIZED");
}

#[test]
fn ops_list_default_output_is_unchanged_by_activity_flag_addition() {
    let root = TestDir::new("ops-list-default");
    write_minimal_registry_state(root.path(), 1);

    let env = run_loom_ok(root.path(), &["ops", "list"]);

    let data = &env["data"];
    assert_eq!(data["state_model"], "registry");
    assert!(data["ops"].is_array(), "{env}");
    assert!(data["operation_counts"].is_object(), "{env}");
    assert!(
        data.get("operations").is_none(),
        "default ops list must keep the backlog shape: {env}"
    );
}
