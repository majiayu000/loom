mod common;

use std::fs;
use std::process::Command;

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
    assert_eq!(env["data"]["initialized"], Value::Bool(true));
    assert_eq!(env["data"]["scanned"], Value::Bool(true));
    assert_eq!(
        env["data"]["imported"].as_array().map(|a| a.len()),
        Some(2),
        "expected both dirs imported: {:?}",
        env["data"]
    );
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(0));

    // Confirm the targets are actually persisted.
    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success());
    assert_eq!(list_env["data"]["count"], Value::from(2));
}

#[test]
fn workspace_init_scan_existing_skips_absent_dirs() {
    let root = TestDir::new("ws-init-scan-skip");
    let fake_home = TestDir::new("ws-init-scan-skip-home");

    // Only create the Claude dir; Codex dir intentionally absent.
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
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(1));
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

    let mut child1 = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(&root_str)
        .args(["workspace", "init", "--scan-existing"])
        .env("HOME", &home_str)
        .spawn()
        .expect("spawn first loom process");

    let mut child2 = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(&root_str)
        .args(["workspace", "init", "--scan-existing"])
        .env("HOME", &home_str)
        .spawn()
        .expect("spawn second loom process");

    let s1 = child1.wait().expect("wait for first process");
    let s2 = child2.wait().expect("wait for second process");

    // At least one must succeed; the other may get LOCK_BUSY.
    assert!(
        s1.success() || s2.success(),
        "neither concurrent init succeeded"
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
