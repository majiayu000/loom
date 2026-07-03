use std::fs;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, run_loom_with_env, write_skill};

fn rollback_copy_fixture(prefix: &str) -> TestDir {
    let root = TestDir::new(prefix);
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

    let target_path = root.path().join("live/claude-copy");
    let (target_output, target_env) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let (binding_output, binding_env) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-copy",
        target_id,
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");

    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );
    assert!(
        skill_project(root.path(), "model-onboarding", binding_id, Some("copy"))
            .0
            .status
            .success(),
        "project should succeed"
    );
    root
}

#[test]
fn rollback_surfaces_projection_snapshot_load_failure() {
    let root = rollback_copy_fixture("rollback-faulted-snapshot-load");

    let (rollback_output, rollback_env) = run_loom_with_env(
        root.path(),
        &[(
            "LOOM_ROLLBACK_FAULT_INJECT",
            "projection_reconciliation_snapshot_load",
        )],
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert_rollback_success(&rollback_output, &rollback_env);
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    assert_eq!(
        reconciliation["status"],
        Value::String("registry_unavailable".to_string())
    );
    assert_eq!(
        reconciliation["error"]["code"],
        Value::String("REGISTRY_STATE_UNAVAILABLE".to_string())
    );
    assert_eq!(
        reconciliation["next_actions"][0]["type"],
        Value::String("manual_review_required".to_string())
    );
    assert_meta_warning_contains(&rollback_env, "registry snapshot loading failed");
}

#[test]
fn rollback_surfaces_real_projection_snapshot_load_failure() {
    let root = rollback_copy_fixture("rollback-real-snapshot-load-failure");
    fs::write(
        root.path().join("state/registry/projections.json"),
        "not json",
    )
    .expect("corrupt projections snapshot");

    let (rollback_output, rollback_env) = run_loom(
        root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert_rollback_success(&rollback_output, &rollback_env);
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    assert_eq!(
        reconciliation["status"],
        Value::String("registry_unavailable".to_string())
    );
    assert_eq!(
        reconciliation["error"]["code"],
        Value::String("REGISTRY_STATE_UNAVAILABLE".to_string())
    );
    assert_meta_warning_contains(
        &rollback_env,
        "could not record projection observations because registry snapshot loading failed",
    );
    assert_meta_warning_contains(&rollback_env, "registry snapshot loading failed");
}

fn assert_rollback_success(output: &std::process::Output, env: &Value) {
    assert!(
        output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
}

fn assert_meta_warning_contains(env: &Value, needle: &str) {
    let warnings = env["meta"]["warnings"]
        .as_array()
        .expect("meta warnings array");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().is_some_and(|text| text.contains(needle))),
        "expected warning containing {needle:?}: {env}"
    );
}
