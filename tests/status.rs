mod common;

use std::fs;
use std::path::Path;

use serde_json::Value;

use common::{TestDir, run_loom, write_minimal_v3_state};

fn overwrite_schema_version(path: &Path, version: u32) {
    let raw = fs::read_to_string(path).expect("read v3 file");
    let updated = raw.replacen(
        "\"schema_version\":3",
        &format!("\"schema_version\":{}", version),
        1,
    );
    fs::write(path, updated).expect("write v3 file");
}

fn assert_workspace_status_schema_mismatch_for(path_suffix: &str) {
    let root = TestDir::new("v3-status-per-file-mismatch");
    write_minimal_v3_state(root.path(), 3);
    overwrite_schema_version(&root.path().join(path_suffix), 99);

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("SCHEMA_MISMATCH".to_string())
    );
}

#[test]
fn workspace_status_reports_v3_snapshot_when_present() {
    let root = TestDir::new("v3-status-ok");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["state_model"], Value::String("v3".to_string()));
    assert_eq!(env["data"]["v3"]["counts"]["bindings"], Value::from(1));
    assert_eq!(env["data"]["v3"]["counts"]["targets"], Value::from(1));
    assert_eq!(
        env["data"]["v3"]["bindings"][0]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
}

#[test]
fn workspace_status_succeeds_when_v3_state_is_missing() {
    let root = TestDir::new("v3-status-missing");

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["state_model"], Value::String("v3".to_string()));
    assert_eq!(env["data"]["v3"]["available"], Value::Bool(false));
}

#[test]
fn workspace_status_fails_with_schema_mismatch_for_invalid_v3_state() {
    let root = TestDir::new("v3-status-bad-schema");
    write_minimal_v3_state(root.path(), 99);

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("SCHEMA_MISMATCH".to_string())
    );
}

#[test]
fn workspace_status_schema_mismatch_schema_file() {
    assert_workspace_status_schema_mismatch_for("state/v3/schema.json");
}

#[test]
fn workspace_status_schema_mismatch_targets_file() {
    assert_workspace_status_schema_mismatch_for("state/v3/targets.json");
}

#[test]
fn workspace_status_schema_mismatch_bindings_file() {
    assert_workspace_status_schema_mismatch_for("state/v3/bindings.json");
}

#[test]
fn workspace_status_schema_mismatch_rules_file() {
    assert_workspace_status_schema_mismatch_for("state/v3/rules.json");
}

#[test]
fn workspace_status_schema_mismatch_projections_file() {
    assert_workspace_status_schema_mismatch_for("state/v3/projections.json");
}

#[test]
fn workspace_status_schema_mismatch_checkpoint_file() {
    assert_workspace_status_schema_mismatch_for("state/v3/ops/checkpoint.json");
}

#[test]
fn workspace_binding_list_returns_bindings_from_v3_state() {
    let root = TestDir::new("v3-binding-list");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["state_model"], Value::String("v3".to_string()));
    assert_eq!(env["data"]["count"], Value::from(1));
    assert_eq!(
        env["data"]["bindings"][0]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
}

#[test]
fn workspace_binding_show_returns_related_target_rules_and_projections() {
    let root = TestDir::new("v3-binding-show");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(
        root.path(),
        &["workspace", "binding", "show", "bind_claude_project_a"],
    );
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["binding"]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
    assert_eq!(
        env["data"]["default_target"]["target_id"],
        Value::String("target_claude_project_a".to_string())
    );
    assert_eq!(env["data"]["rules"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["data"]["projections"].as_array().map(Vec::len), Some(1));
}

#[test]
fn workspace_binding_show_fails_for_unknown_binding() {
    let root = TestDir::new("v3-binding-missing");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "show", "missing"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("BINDING_NOT_FOUND".to_string())
    );
}

#[test]
fn target_list_returns_targets_from_v3_state() {
    let root = TestDir::new("v3-target-list");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(root.path(), &["target", "list"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["state_model"], Value::String("v3".to_string()));
    assert_eq!(env["data"]["count"], Value::from(1));
    assert_eq!(
        env["data"]["targets"][0]["target_id"],
        Value::String("target_claude_project_a".to_string())
    );
}

#[test]
fn target_show_returns_related_bindings_rules_and_projections() {
    let root = TestDir::new("v3-target-show");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(root.path(), &["target", "show", "target_claude_project_a"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["target"]["target_id"],
        Value::String("target_claude_project_a".to_string())
    );
    assert_eq!(env["data"]["bindings"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["data"]["rules"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["data"]["projections"].as_array().map(Vec::len), Some(1));
}

#[test]
fn target_show_fails_for_unknown_target() {
    let root = TestDir::new("v3-target-missing");
    write_minimal_v3_state(root.path(), 3);

    let (output, env) = run_loom(root.path(), &["target", "show", "missing"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("TARGET_NOT_FOUND".to_string())
    );
}

#[test]
fn workspace_binding_commands_fail_cleanly_without_v3_state() {
    let root = TestDir::new("v3-binding-no-state");

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}
