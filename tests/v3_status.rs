use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("loom-{}-{}", prefix, Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn loom_bin() -> &'static str {
    env!("CARGO_BIN_EXE_loom")
}

fn run_loom(root: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(loom_bin())
        .arg("--json")
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, body).expect("write file");
}

fn write_minimal_v3_state(root: &Path, schema_version: u32) {
    let v3 = root.join("state/v3");
    write_file(
        &v3.join("schema.json"),
        &format!(
            "{{\"schema_version\":{},\"created_at\":\"2026-04-09T10:00:00Z\",\"writer\":\"loom/3.0.0-draft\"}}\n",
            schema_version
        ),
    );
    write_file(
        &v3.join("targets.json"),
        r#"{"schema_version":3,"targets":[{"target_id":"target_claude_project_a","agent":"claude","path":"/tmp/claude-a/skills","ownership":"managed","capabilities":{"symlink":true,"copy":true,"watch":true},"created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &v3.join("bindings.json"),
        r#"{"schema_version":3,"bindings":[{"binding_id":"bind_claude_project_a","agent":"claude","profile_id":"default","workspace_matcher":{"kind":"path_prefix","value":"/tmp/project-a"},"default_target_id":"target_claude_project_a","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &v3.join("rules.json"),
        r#"{"schema_version":3,"rules":[{"binding_id":"bind_claude_project_a","skill_id":"model-onboarding","target_id":"target_claude_project_a","method":"symlink","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &v3.join("projections.json"),
        r#"{"schema_version":3,"projections":[{"instance_id":"inst_model-onboarding_claude_a","skill_id":"model-onboarding","binding_id":"bind_claude_project_a","target_id":"target_claude_project_a","materialized_path":"/tmp/claude-a/skills/model-onboarding","method":"symlink","last_applied_rev":"abc123","health":"healthy","observed_drift":false,"updated_at":"2026-04-09T10:05:00Z"}]}
"#,
    );
    write_file(
        &v3.join("ops/checkpoint.json"),
        r#"{"schema_version":3,"last_scanned_op_id":"op_001","last_acked_op_id":null,"updated_at":"2026-04-09T10:07:00Z"}
"#,
    );
    write_file(
        &v3.join("ops/operations.jsonl"),
        r#"{"op_id":"op_001","intent":"skill.project","status":"succeeded","ack":false,"payload":{"skill_id":"model-onboarding","binding_id":"bind_claude_project_a"},"effects":{"instance_id":"inst_model-onboarding_claude_a"},"created_at":"2026-04-09T10:05:00Z","updated_at":"2026-04-09T10:05:00Z"}
"#,
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
