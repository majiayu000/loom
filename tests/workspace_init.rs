mod common;

use std::fs;
use std::process::{Command, Stdio};

use common::{TestDir, run_loom, run_loom_with_env};
use serde_json::Value;

// Exercises the reentrant-lock production path: cmd_workspace_init holds the
// workspace lock at line 159 and then calls cmd_target_add (line 200) which
// re-acquires the same lock.  If reentrancy is broken this hangs or panics.
#[test]
fn workspace_init_scan_existing_imports_present_dirs() {
    let root = TestDir::new("ws-init-scan-import");
    let fake_home = TestDir::new("ws-init-scan-import-home");

    fs::create_dir_all(fake_home.path().join(".claude/skills")).expect("create .claude/skills");
    fs::create_dir_all(fake_home.path().join(".codex/skills")).expect("create .codex/skills");
    fs::create_dir_all(fake_home.path().join(".cursor/skills")).expect("create .cursor/skills");

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(
        output.status.success(),
        "workspace init --scan-existing failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["meta"].get("op_id"), None);
    assert_eq!(env["data"]["initialized"], Value::Bool(true));
    assert_eq!(env["data"]["scanned"], Value::Bool(true));
    assert_eq!(
        env["data"]["imported"].as_array().map(|a| a.len()),
        Some(3),
        "expected present default dirs imported: {:?}",
        env["data"]
    );
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(7));

    // Confirm the targets are actually persisted.
    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success());
    assert_eq!(list_env["data"]["count"], Value::from(3));
}

#[test]
fn workspace_init_scan_existing_skips_absent_dirs() {
    let root = TestDir::new("ws-init-scan-skip");
    let fake_home = TestDir::new("ws-init-scan-skip-home");

    // Only create the Claude dir; all other default agent dirs intentionally absent.
    fs::create_dir_all(fake_home.path().join(".claude/skills")).expect("create .claude/skills");

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["data"]["imported"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(9));
    assert_eq!(
        env["data"]["skipped"][0]["reason"],
        Value::String("does-not-exist".to_string())
    );
}

// Two processes race to `workspace init --scan-existing` on the same root.
// The second process will get LOCK_BUSY (the filesystem lock is non-blocking).
// After both finish the state must not be corrupted: exactly two targets should
// exist (idempotency + reentrancy both hold).
#[test]
fn workspace_init_scan_existing_concurrent_inits_leave_consistent_state() {
    let root = TestDir::new("ws-init-concurrent");
    let fake_home = TestDir::new("ws-init-concurrent-home");

    fs::create_dir_all(fake_home.path().join(".claude/skills")).expect("create .claude/skills");
    fs::create_dir_all(fake_home.path().join(".codex/skills")).expect("create .codex/skills");

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let root_str = root.path().to_string_lossy().into_owned();

    let child1 = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(&root_str)
        .args(["workspace", "init", "--scan-existing"])
        .env("HOME", &home_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first loom process");

    let child2 = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(&root_str)
        .args(["workspace", "init", "--scan-existing"])
        .env("HOME", &home_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn second loom process");

    let out1 = child1.wait_with_output().expect("wait for first process");
    let out2 = child2.wait_with_output().expect("wait for second process");

    // At least one must succeed; the other may get LOCK_BUSY.
    assert!(
        out1.status.success() || out2.status.success(),
        "neither concurrent init succeeded: stderr1={} stderr2={}",
        String::from_utf8_lossy(&out1.stderr),
        String::from_utf8_lossy(&out2.stderr)
    );

    // State must be consistent regardless of which process won the race.
    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(
        list_output.status.success(),
        "target list failed after concurrent inits: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    assert_eq!(
        list_env["data"]["count"],
        Value::from(2),
        "expected exactly 2 targets after concurrent inits"
    );
}
