mod common;

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_minimal_registry_state};

fn overwrite_schema_version(path: &Path, version: u32) {
    let raw = fs::read_to_string(path).expect("read registry file");
    let updated = raw.replacen(
        "\"schema_version\":1",
        &format!("\"schema_version\":{}", version),
        1,
    );
    fs::write(path, updated).expect("write registry file");
}

fn assert_workspace_status_schema_mismatch_for(path_suffix: &str) {
    let root = TestDir::new("registry-status-per-file-mismatch");
    write_minimal_registry_state(root.path(), 1);
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
fn workspace_status_reports_registry_snapshot_when_present() {
    let root = TestDir::new("registry-status-ok");
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["state_model"],
        Value::String("registry".to_string())
    );
    assert_eq!(
        env["data"]["registry"]["counts"]["bindings"],
        Value::from(1)
    );
    assert_eq!(env["data"]["registry"]["counts"]["targets"], Value::from(1));
    assert_eq!(
        env["data"]["registry"]["bindings"][0]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
    assert_eq!(
        env["data"]["registry"]["targets"][0]["agent_source"],
        Value::String("built-in".to_string())
    );
}

#[test]
fn workspace_status_marks_legacy_copy_projection_not_observed_without_counting_drift() {
    let root = TestDir::new("registry-status-legacy-projection-observation");
    write_minimal_registry_state(root.path(), 1);
    let projections_path = root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_str(&fs::read_to_string(&projections_path).expect("read projections"))
            .expect("parse projections");
    projections["projections"][0]["method"] = json!("copy");
    fs::write(
        &projections_path,
        serde_json::to_string_pretty(&projections).expect("serialize projections"),
    )
    .expect("write projections");

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(
        env["data"]["registry"]["projections"][0]["observation_status"],
        json!("not_observed")
    );
    assert_eq!(
        env["data"]["registry"]["counts"]["drifted_projections"],
        json!(0)
    );
}

#[test]
fn workspace_status_does_not_migrate_pre_release_state_dir() {
    let root = TestDir::new("registry-status-read-does-not-migrate-old-state-dir");
    write_minimal_registry_state(root.path(), 1);
    fs::rename(
        root.path().join("state/registry"),
        root.path().join("state/v3"),
    )
    .expect("move registry state to old pre-release path");

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["registry"]["available"], Value::Bool(false));
    assert_eq!(
        env["data"]["registry"]["error"]["code"],
        Value::String("SCHEMA_MISMATCH".to_string())
    );
    assert!(!root.path().join("state/registry/schema.json").exists());
    assert!(root.path().join("state/v3/schema.json").exists());
}

#[test]
fn workspace_status_succeeds_when_registry_state_is_missing() {
    let root = TestDir::new("registry-status-missing");

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["state_model"],
        Value::String("registry".to_string())
    );
    assert_eq!(env["data"]["registry"]["available"], Value::Bool(false));
    assert_eq!(
        env["data"]["registry"]["error"]["code"],
        Value::String("STATE_NOT_INITIALIZED".to_string())
    );
}

#[test]
fn workspace_status_reports_all_supported_agent_dir_defaults() {
    let root = TestDir::new("registry-status-agent-dirs");
    let fake_home = TestDir::new("registry-status-agent-dirs-home");
    write_file(
        &root.path().join(".env"),
        "OPENCODE_SKILLS_DIR=/tmp/opencode-primary,/tmp/opencode-secondary\n",
    );

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "status"],
    );
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let agent_dirs = env["data"]["agent_dir_defaults"]["agent_dirs"]
        .as_array()
        .expect("agent_dirs must be an array");
    assert_eq!(agent_dirs.len(), 10);

    let path_for = |agent: &str| {
        agent_dirs
            .iter()
            .find(|dir| dir["agent"] == Value::String(agent.to_string()))
            .and_then(|dir| dir["path"].as_str())
            .unwrap_or_else(|| panic!("missing agent dir for {agent}"))
            .to_string()
    };
    assert_eq!(
        path_for("claude"),
        fake_home
            .path()
            .join(".claude/skills")
            .display()
            .to_string()
    );
    assert_eq!(
        path_for("copilot"),
        fake_home
            .path()
            .join(".github/copilot/skills")
            .display()
            .to_string()
    );
    assert_eq!(
        path_for("gemini-cli"),
        fake_home
            .path()
            .join(".gemini/skills")
            .display()
            .to_string()
    );
    assert_eq!(path_for("opencode"), "/tmp/opencode-primary");

    let source_dirs = env["data"]["inventory"]["source_dirs"]
        .as_array()
        .expect("source_dirs must be an array")
        .iter()
        .filter_map(|path| path.as_str())
        .collect::<Vec<_>>();
    assert!(source_dirs.contains(&"/tmp/opencode-primary"));
    assert!(source_dirs.contains(&"/tmp/opencode-secondary"));
    assert!(
        source_dirs
            .iter()
            .any(|path| path.ends_with(".config/goose/skills"))
    );
}

#[test]
fn workspace_status_fails_with_schema_mismatch_for_invalid_registry_state() {
    let root = TestDir::new("registry-status-bad-schema");
    write_minimal_registry_state(root.path(), 99);

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
    assert_workspace_status_schema_mismatch_for("state/registry/schema.json");
}

#[test]
fn workspace_status_schema_mismatch_targets_file() {
    assert_workspace_status_schema_mismatch_for("state/registry/targets.json");
}

#[test]
fn workspace_status_schema_mismatch_bindings_file() {
    assert_workspace_status_schema_mismatch_for("state/registry/bindings.json");
}

#[test]
fn workspace_status_schema_mismatch_rules_file() {
    assert_workspace_status_schema_mismatch_for("state/registry/rules.json");
}

#[test]
fn workspace_status_schema_mismatch_projections_file() {
    assert_workspace_status_schema_mismatch_for("state/registry/projections.json");
}

#[test]
fn workspace_status_schema_mismatch_checkpoint_file() {
    assert_workspace_status_schema_mismatch_for("state/registry/ops/checkpoint.json");
}

#[test]
fn workspace_binding_list_returns_bindings_from_registry_state() {
    let root = TestDir::new("registry-binding-list");
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["state_model"],
        Value::String("registry".to_string())
    );
    assert_eq!(env["data"]["count"], Value::from(1));
    assert_eq!(
        env["data"]["bindings"][0]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
}

#[test]
fn workspace_binding_show_returns_related_target_rules_and_projections() {
    let root = TestDir::new("registry-binding-show");
    write_minimal_registry_state(root.path(), 1);

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
    let root = TestDir::new("registry-binding-missing");
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "show", "missing"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("BINDING_NOT_FOUND".to_string())
    );
}

#[test]
fn target_list_returns_targets_from_registry_state() {
    let root = TestDir::new("registry-target-list");
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(root.path(), &["target", "list"]);
    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["state_model"],
        Value::String("registry".to_string())
    );
    assert_eq!(env["data"]["count"], Value::from(1));
    assert_eq!(
        env["data"]["targets"][0]["target_id"],
        Value::String("target_claude_project_a".to_string())
    );
    assert_eq!(
        env["data"]["targets"][0]["agent_source"],
        Value::String("built-in".to_string())
    );
}

#[test]
fn target_show_returns_related_bindings_rules_and_projections() {
    let root = TestDir::new("registry-target-show");
    write_minimal_registry_state(root.path(), 1);

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
    assert_eq!(
        env["data"]["target"]["agent_source"],
        Value::String("built-in".to_string())
    );
    assert_eq!(env["data"]["bindings"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["data"]["rules"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["data"]["projections"].as_array().map(Vec::len), Some(1));
}

#[test]
fn target_show_fails_for_unknown_target() {
    let root = TestDir::new("registry-target-missing");
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(root.path(), &["target", "show", "missing"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("TARGET_NOT_FOUND".to_string())
    );
}

#[test]
fn workspace_binding_commands_fail_cleanly_without_registry_state() {
    let root = TestDir::new("registry-binding-no-state");

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(!output.status.success(), "loom unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("STATE_NOT_INITIALIZED".to_string())
    );
}
