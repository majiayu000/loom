mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::actions::{binding_add, target_add};
use serde_json::Value;

use common::{TestDir, run_loom, write_minimal_registry_state};

fn git_ok(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_status(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .status()
        .expect("run git")
        .success()
}

#[test]
fn target_add_bootstraps_registry_state_and_records_op() {
    let root = TestDir::new("registry-target-add");
    let target_path = root.path().join("live/claude-project-a");
    let (output, env) = target_add(root.path(), "claude", &target_path, "managed");

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
    assert!(root.path().join("state/registry/schema.json").exists());
}

#[test]
fn target_add_is_idempotent_for_same_agent_and_path() {
    let root = TestDir::new("registry-target-add-idempotent");
    let target_path = root.path().join("live/codex-workbench");
    let (first_output, _) = target_add(root.path(), "codex", &target_path, "managed");
    assert!(first_output.status.success(), "first add should succeed");

    let (second_output, second_env) = target_add(root.path(), "codex", &target_path, "managed");
    assert!(second_output.status.success(), "second add should succeed");
    assert_eq!(second_env["data"]["noop"], Value::Bool(true));

    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success(), "target list should succeed");
    assert_eq!(list_env["data"]["count"], Value::from(1));
}

#[test]
fn workspace_binding_add_uses_existing_target_and_records_op() {
    let root = TestDir::new("registry-binding-add");
    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, binding_env) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_claude_project_a",
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
    let root = TestDir::new("registry-binding-add-missing-target");

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

#[test]
fn workspace_binding_add_rejects_malformed_policy_profile() {
    let root = TestDir::new("registry-binding-bad-policy");
    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");

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
            "target_claude_claude_project_a",
            "--policy-profile",
            "Total Nonsense",
        ],
    );

    assert!(
        !output.status.success(),
        "binding add unexpectedly accepted malformed policy profile"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}

#[test]
fn registry_state_commit_stages_legacy_v3_deletions() {
    let root = TestDir::new("registry-state-stage-v3-delete");
    let target_path = root.path().join("live/claude");
    write_minimal_registry_state(root.path(), 1);
    fs::rename(
        root.path().join("state/registry"),
        root.path().join("state/v3"),
    )
    .expect("move registry state to old pre-release path");

    git_ok(root.path(), &["init", "-b", "main"]);
    git_ok(
        root.path(),
        &["config", "--local", "user.name", "loom-test"],
    );
    git_ok(
        root.path(),
        &["config", "--local", "user.email", "loom-test@example.com"],
    );
    git_ok(root.path(), &["add", "state/v3"]);
    git_ok(root.path(), &["commit", "-m", "legacy registry state"]);
    assert!(git_status(
        root.path(),
        &["cat-file", "-e", "HEAD:state/v3/schema.json"]
    ));

    let (output, env) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(
        output.status.success(),
        "target add failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));

    assert!(git_status(
        root.path(),
        &["cat-file", "-e", "HEAD:state/registry/schema.json"]
    ));
    assert!(
        !git_status(
            root.path(),
            &["cat-file", "-e", "HEAD:state/v3/schema.json"]
        ),
        "legacy state/v3 should be deleted from HEAD"
    );
    let status = git_ok(root.path(), &["status", "--short"]);
    assert!(
        !status.contains("state/v3"),
        "legacy state/v3 should not remain dirty: {status}"
    );
}

#[test]
fn target_add_uses_parent_context_for_generic_skills_leaf() {
    let root = TestDir::new("registry-target-add-generic-skills-leaf");
    let claude_path = root.path().join("agent/.claude/skills");
    let claude_work_path = root.path().join("agent/.claude-work/skills");

    let (a_output, a_env) = target_add(root.path(), "claude", &claude_path, "managed");
    assert!(
        a_output.status.success(),
        "first target add failed: stderr={} stdout={}",
        String::from_utf8_lossy(&a_output.stderr),
        String::from_utf8_lossy(&a_output.stdout)
    );
    assert_eq!(
        a_env["data"]["target"]["target_id"],
        Value::String("target_claude_claude_skills".to_string())
    );

    let (b_output, b_env) = target_add(root.path(), "claude", &claude_work_path, "managed");
    assert!(
        b_output.status.success(),
        "second target add failed: stderr={} stdout={}",
        String::from_utf8_lossy(&b_output.stderr),
        String::from_utf8_lossy(&b_output.stdout)
    );
    assert_eq!(
        b_env["data"]["target"]["target_id"],
        Value::String("target_claude_claude_work_skills".to_string())
    );
}
