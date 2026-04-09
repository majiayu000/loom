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

#[test]
fn target_add_bootstraps_v3_state_and_records_op() {
    let root = TestDir::new("v3-target-add");
    let target_path = root.path().join("live/claude-project-a");
    let target_path_str = target_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
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

    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["noop"], Value::Bool(false));
    assert_eq!(
        env["data"]["target"]["target_id"],
        Value::String("target_claude_claude_project_a".to_string())
    );
    assert_eq!(
        env["meta"]["op_id"].as_str().map(|value| !value.is_empty()),
        Some(true)
    );
    assert!(
        target_path.exists(),
        "managed target path should be created"
    );
    assert!(root.path().join("state/v3/schema.json").exists());
}

#[test]
fn target_add_is_idempotent_for_same_agent_and_path() {
    let root = TestDir::new("v3-target-add-idempotent");
    let target_path = root.path().join("live/codex-workbench");
    let target_path_str = target_path.to_string_lossy().to_string();

    let (first_output, _) = run_loom(
        root.path(),
        &[
            "target",
            "add",
            "--agent",
            "codex",
            "--path",
            &target_path_str,
            "--ownership",
            "managed",
        ],
    );
    assert!(first_output.status.success(), "first add should succeed");

    let (second_output, second_env) = run_loom(
        root.path(),
        &[
            "target",
            "add",
            "--agent",
            "codex",
            "--path",
            &target_path_str,
            "--ownership",
            "managed",
        ],
    );
    assert!(second_output.status.success(), "second add should succeed");
    assert_eq!(second_env["data"]["noop"], Value::Bool(true));

    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success(), "target list should succeed");
    assert_eq!(list_env["data"]["count"], Value::from(1));
}

#[test]
fn workspace_binding_add_uses_existing_target_and_records_op() {
    let root = TestDir::new("v3-binding-add");
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

    let (binding_output, binding_env) = run_loom(
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
        "binding add failed: stderr={} stdout={}",
        String::from_utf8_lossy(&binding_output.stderr),
        String::from_utf8_lossy(&binding_output.stdout)
    );
    assert_eq!(binding_env["ok"], Value::Bool(true));
    assert_eq!(binding_env["data"]["noop"], Value::Bool(false));
    assert_eq!(
        binding_env["data"]["binding"]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
    assert_eq!(
        binding_env["meta"]["op_id"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );

    let (show_output, show_env) = run_loom(
        root.path(),
        &["workspace", "binding", "show", "bind_claude_project_a"],
    );
    assert!(show_output.status.success(), "binding show should succeed");
    assert_eq!(
        show_env["data"]["default_target"]["target_id"],
        Value::String("target_claude_claude_project_a".to_string())
    );
}

#[test]
fn workspace_binding_add_fails_for_unknown_target() {
    let root = TestDir::new("v3-binding-add-missing-target");

    let (output, env) = run_loom(
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
            "missing_target",
        ],
    );

    assert!(
        !output.status.success(),
        "binding add unexpectedly succeeded"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("TARGET_NOT_FOUND".to_string())
    );
}
