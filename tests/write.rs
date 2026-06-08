mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use common::actions::{binding_add, target_add, target_add_with_default_ownership};
use serde_json::Value;

use common::{
    TestDir, operations_log, run_loom, run_loom_with_env, write_minimal_registry_state, write_skill,
};

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
    // Suppress stderr; callers intentionally probe for missing paths
    // (e.g. `cat-file -e HEAD:...`) where git logs "fatal: ..." on a clean miss.
    Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run git")
        .success()
}

fn registry_array_len(root: &Path, file_name: &str, key: &str) -> usize {
    let path = root.join("state/registry").join(file_name);
    if !path.exists() {
        return 0;
    }
    let raw = fs::read_to_string(&path).expect("read registry json");
    let value: Value = serde_json::from_str(&raw).expect("parse registry json");
    value[key].as_array().map(Vec::len).unwrap_or(0)
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
fn skill_save_records_registry_operation_and_op_id() {
    let root = TestDir::new("registry-skill-save-op-id");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");

    let (output, env) = run_loom(root.path(), &["skill", "save", "demo"]);

    assert!(
        output.status.success(),
        "save failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    let op_id = env["meta"]["op_id"].as_str().expect("save op_id");
    assert!(op_id.starts_with("op_"), "unexpected op_id: {op_id}");
    let operations = operations_log(root.path());
    assert!(operations.contains(&format!(r#""op_id":"{op_id}""#)));
    assert!(operations.contains(r#""intent":"skill.save""#));
    assert!(operations.contains(r#""skill":"demo""#));
}

#[test]
fn skill_save_rolls_back_registry_operation_after_audit_failure() {
    let root = TestDir::new("registry-skill-save-audit-rollback");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_save_after_operation")],
        &["skill", "save", "demo"],
    );

    assert!(!output.status.success(), "save unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert!(
        !root.path().join("state/registry").exists(),
        "fresh registry layout should be removed after failed save audit"
    );
    assert!(
        git_ok(root.path(), &["log", "--oneline", "--", "skills/demo"]).is_empty(),
        "skill commit should be rolled back"
    );
}

#[test]
fn skill_save_restores_legacy_v3_layout_after_audit_failure() {
    let root = TestDir::new("registry-skill-save-legacy-v3-audit-rollback");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (initial_save, _) = run_loom(root.path(), &["skill", "save", "demo"]);
    assert!(
        initial_save.status.success(),
        "initial save failed: stderr={} stdout={}",
        String::from_utf8_lossy(&initial_save.stderr),
        String::from_utf8_lossy(&initial_save.stdout)
    );

    fs::rename(
        root.path().join("state/registry"),
        root.path().join("state/v3"),
    )
    .expect("move registry state to legacy v3");
    git_ok(
        root.path(),
        &["add", "-A", "--", "state/registry", "state/v3"],
    );
    git_ok(
        root.path(),
        &[
            "commit",
            "-m",
            "legacy registry layout",
            "--",
            "state/registry",
            "state/v3",
        ],
    );
    assert!(root.path().join("state/v3").exists());
    assert!(!root.path().join("state/registry").exists());
    assert_eq!(
        git_ok(
            root.path(),
            &["status", "--short", "--", "state/registry", "state/v3"]
        ),
        ""
    );
    let legacy_ops_before = fs::read_to_string(root.path().join("state/v3/ops/operations.jsonl"))
        .expect("read legacy ops");
    let head_before = git_ok(root.path(), &["rev-parse", "HEAD"]);

    write_skill(root.path(), "demo", "# demo\n\nv2\n");
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_save_after_operation")],
        &["skill", "save", "demo"],
    );

    assert!(!output.status.success(), "save unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(git_ok(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert!(root.path().join("state/v3").exists());
    assert!(
        !root.path().join("state/registry").exists(),
        "failed save should restore legacy v3 instead of keeping migrated registry"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/v3/ops/operations.jsonl"))
            .expect("read legacy ops"),
        legacy_ops_before
    );
    assert_eq!(
        git_ok(
            root.path(),
            &["status", "--short", "--", "state/registry", "state/v3"]
        ),
        ""
    );
}

#[test]
fn skill_save_restores_legacy_v3_layout_after_layout_failure() {
    let root = TestDir::new("registry-skill-save-legacy-v3-layout-rollback");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (initial_save, _) = run_loom(root.path(), &["skill", "save", "demo"]);
    assert!(
        initial_save.status.success(),
        "initial save failed: stderr={} stdout={}",
        String::from_utf8_lossy(&initial_save.stderr),
        String::from_utf8_lossy(&initial_save.stdout)
    );

    fs::rename(
        root.path().join("state/registry"),
        root.path().join("state/v3"),
    )
    .expect("move registry state to legacy v3");
    let legacy_ops_path = root.path().join("state/v3/ops/operations.jsonl");
    fs::remove_file(&legacy_ops_path).expect("remove legacy operations log");
    fs::create_dir(&legacy_ops_path).expect("make legacy operations log path corrupt");
    git_ok(
        root.path(),
        &["add", "-A", "--", "state/registry", "state/v3"],
    );
    git_ok(
        root.path(),
        &[
            "commit",
            "-m",
            "corrupt legacy registry layout",
            "--",
            "state/registry",
            "state/v3",
        ],
    );
    assert!(root.path().join("state/v3").exists());
    assert!(legacy_ops_path.is_dir());
    assert!(!root.path().join("state/registry").exists());
    let head_before = git_ok(root.path(), &["rev-parse", "HEAD"]);

    write_skill(root.path(), "demo", "# demo\n\nv2\n");
    let (output, env) = run_loom(root.path(), &["skill", "save", "demo"]);

    assert!(!output.status.success(), "save unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(git_ok(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert!(root.path().join("state/v3").exists());
    assert!(root.path().join("state/v3/ops/operations.jsonl").is_dir());
    assert!(
        !root.path().join("state/registry").exists(),
        "failed layout migration should restore legacy v3 instead of keeping migrated registry"
    );
    assert_eq!(
        git_ok(
            root.path(),
            &["status", "--short", "--", "state/registry", "state/v3"]
        ),
        ""
    );
}

#[test]
fn skill_snapshot_without_registry_operation_does_not_return_op_id() {
    let root = TestDir::new("registry-skill-snapshot-no-fake-op-id");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    assert!(
        run_loom(root.path(), &["skill", "save", "demo"])
            .0
            .status
            .success()
    );

    let (output, env) = run_loom(root.path(), &["skill", "snapshot", "demo"]);

    assert!(
        output.status.success(),
        "snapshot failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["meta"].get("op_id"), None);
    assert!(!operations_log(root.path()).contains("\"intent\":\"skill.snapshot\""));
}

#[test]
fn target_add_defaults_to_observed_ownership() {
    let root = TestDir::new("registry-target-add-default-observed");
    let target_path = root.path().join("live/claude-project-a");
    fs::create_dir_all(&target_path).expect("create observed target path");

    let (output, env) = target_add_with_default_ownership(root.path(), "claude", &target_path);

    assert!(
        output.status.success(),
        "loom failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["target"]["ownership"],
        Value::String("observed".to_string())
    );
    assert_eq!(
        env["data"]["target"]["capabilities"]["symlink"],
        Value::Bool(false)
    );
    assert_eq!(
        env["data"]["target"]["capabilities"]["copy"],
        Value::Bool(false)
    );
    assert_eq!(
        env["data"]["target"]["capabilities"]["watch"],
        Value::Bool(true)
    );
}

#[test]
fn target_add_rejects_directory_operations_log_before_mutating_targets() {
    let root = TestDir::new("registry-target-add-directory-oplog");
    fs::create_dir_all(root.path().join("state/registry/ops/operations.jsonl"))
        .expect("create directory at operations log path");
    let target_path = root.path().join("live/claude-project-a");

    let (output, env) = target_add(root.path(), "claude", &target_path, "managed");

    assert!(
        !output.status.success(),
        "target add unexpectedly succeeded with directory operations log"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("STATE_CORRUPT".to_string())
    );
    assert_eq!(
        registry_array_len(root.path(), "targets.json", "targets"),
        0,
        "failed command must not leave the target in registry state"
    );
}

#[test]
fn target_add_rolls_back_targets_after_operation_log_failure() {
    let root = TestDir::new("registry-target-add-oplog-rollback");
    let target_path = root.path().join("live/claude-project-a");
    let target_path_arg = target_path.to_string_lossy().into_owned();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_append")],
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            &target_path_arg,
            "--ownership",
            "managed",
        ],
    );

    assert!(
        !output.status.success(),
        "target add unexpectedly succeeded with injected operation-log failure"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        registry_array_len(root.path(), "targets.json", "targets"),
        0,
        "target state must roll back when the operation record cannot be persisted"
    );
    assert!(
        operations_log(root.path()).is_empty(),
        "operation log append must also be rolled back"
    );
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
fn target_add_is_idempotent_for_equivalent_directory_path() {
    let root = TestDir::new("registry-target-add-equivalent-path");
    let target_path = root.path().join("live/codex-workbench");
    let (first_output, first_env) = target_add(root.path(), "codex", &target_path, "managed");
    assert!(first_output.status.success(), "first add should succeed");

    let equivalent_path = target_path.join(".");
    let (second_output, second_env) = target_add(root.path(), "codex", &equivalent_path, "managed");
    assert!(second_output.status.success(), "second add should succeed");
    assert_eq!(second_env["data"]["noop"], Value::Bool(true));

    let canonical_path = target_path
        .canonicalize()
        .expect("canonicalize managed target")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        first_env["data"]["target"]["path"],
        Value::from(canonical_path)
    );

    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success(), "target list should succeed");
    assert_eq!(list_env["data"]["count"], Value::from(1));
}

#[cfg(unix)]
#[test]
fn target_add_is_idempotent_for_symlinked_directory_path() {
    use std::os::unix::fs::symlink;

    let root = TestDir::new("registry-target-add-symlink-path");
    let target_path = root.path().join("live/codex-workbench");
    let link_path = root.path().join("live/codex-link");
    fs::create_dir_all(&target_path).expect("create observed target");
    symlink(&target_path, &link_path).expect("create target symlink");

    let (first_output, _) = target_add(root.path(), "codex", &target_path, "observed");
    assert!(first_output.status.success(), "first add should succeed");

    let (second_output, second_env) = target_add(root.path(), "codex", &link_path, "observed");
    assert!(second_output.status.success(), "second add should succeed");
    assert_eq!(second_env["data"]["noop"], Value::Bool(true));

    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success(), "target list should succeed");
    assert_eq!(list_env["data"]["count"], Value::from(1));
}

#[test]
fn workspace_binding_add_rolls_back_bindings_after_operation_log_failure() {
    let root = TestDir::new("registry-binding-add-oplog-rollback");
    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let operations_before = operations_log(root.path());

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_append")],
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
        !output.status.success(),
        "binding add unexpectedly succeeded with injected operation-log failure"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        registry_array_len(root.path(), "bindings.json", "bindings"),
        0,
        "binding state must roll back when the operation record cannot be persisted"
    );
    assert_eq!(
        operations_log(root.path()),
        operations_before,
        "operation log should be restored to the pre-command contents"
    );
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
