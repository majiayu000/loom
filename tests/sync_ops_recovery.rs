mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use common::{TestDir, run_loom};

fn run_loom_ok(root: &Path, args: &[&str]) -> Value {
    let (output, env) = run_loom(root, args);
    assert!(
        output.status.success(),
        "loom {args:?} failed: {env} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    env
}

fn git_ok(args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn make_skill_source(root: &Path, name: &str) -> PathBuf {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill source dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Use when testing sync recovery paths.\n---\n# {name}\n"
        ),
    )
    .expect("write SKILL.md");
    skill_dir
}

fn init_bare_remote(holder: &TestDir) -> String {
    let remote = holder.path().join("origin.git");
    git_ok(&["init", "--bare", remote.to_str().expect("remote path")]);
    remote.to_string_lossy().into_owned()
}

/// Queue one registry operation by importing a skill while the configured
/// remote is unreachable, leaving the workspace in PENDING_PUSH.
fn queue_operation_behind_broken_remote(root: &Path) {
    let source = make_skill_source(root, "source-demo");
    let missing_remote = root.join("missing-remote.git");
    run_loom_ok(
        root,
        &[
            "workspace",
            "remote",
            "set",
            missing_remote.to_str().expect("remote path"),
        ],
    );
    let add = run_loom_ok(
        root,
        &[
            "skill",
            "add",
            source.to_str().expect("source path"),
            "--name",
            "demo",
        ],
    );
    assert_eq!(add["meta"]["sync_state"], json!("PENDING_PUSH"));
    let pending = run_loom_ok(root, &["ops", "list"]);
    assert_eq!(pending["data"]["count"], json!(1));
}

fn repoint_remote(root: &Path, remote: &str) {
    run_loom_ok(root, &["workspace", "remote", "set", remote]);
}

#[test]
fn sync_replay_reports_no_operations_on_empty_backlog() {
    let root = TestDir::new("sync-replay-empty");
    run_loom_ok(root.path(), &["workspace", "init"]);

    let env = run_loom_ok(root.path(), &["sync", "replay"]);

    assert_eq!(env["ok"], json!(true));
    assert_eq!(env["cmd"], json!("sync.replay"));
    assert_eq!(env["data"]["result"], json!("no_operations"));
}

#[test]
fn sync_replay_drains_queued_operations_after_remote_recovery() {
    let root = TestDir::new("sync-replay-recover");
    let remote_holder = TestDir::new("sync-replay-recover-remote");
    queue_operation_behind_broken_remote(root.path());
    let remote = init_bare_remote(&remote_holder);
    repoint_remote(root.path(), &remote);

    let env = run_loom_ok(root.path(), &["sync", "replay"]);

    assert_eq!(env["data"]["result"], json!("replayed"));
    let pending = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(
        pending["data"]["count"],
        json!(0),
        "backlog should drain after replay: {pending}"
    );
    let remote_main = git_ok(&["--git-dir", &remote, "rev-parse", "main"]);
    assert!(
        !remote_main.is_empty(),
        "replay should push registry state to the recovered remote"
    );
}

#[test]
fn sync_replay_keeps_backlog_when_remote_is_unreachable() {
    let root = TestDir::new("sync-replay-unreachable");
    queue_operation_behind_broken_remote(root.path());

    let (output, env) = run_loom(root.path(), &["sync", "replay"]);

    assert!(!output.status.success(), "replay unexpectedly succeeded");
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("REMOTE_UNREACHABLE"));
    let pending = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(
        pending["data"]["count"],
        json!(1),
        "failed replay must not drop the queued operation: {pending}"
    );
}

#[test]
fn ops_retry_reports_empty_queue_without_remote_contact() {
    let root = TestDir::new("ops-retry-empty");
    run_loom_ok(root.path(), &["workspace", "init"]);

    let env = run_loom_ok(root.path(), &["ops", "retry"]);

    assert_eq!(env["ok"], json!(true));
    assert_eq!(env["cmd"], json!("ops.retry"));
    assert_eq!(env["data"]["result"], json!("no_operations"));
    assert_eq!(env["data"]["queued_before"], json!(0));
    assert_eq!(env["data"]["queued_after"], json!(0));
}

#[test]
fn ops_retry_drains_backlog_and_reports_queue_counts() {
    let root = TestDir::new("ops-retry-recover");
    let remote_holder = TestDir::new("ops-retry-recover-remote");
    queue_operation_behind_broken_remote(root.path());
    let remote = init_bare_remote(&remote_holder);
    repoint_remote(root.path(), &remote);

    let env = run_loom_ok(root.path(), &["ops", "retry"]);

    assert_eq!(env["data"]["result"], json!("replayed"));
    assert_eq!(env["data"]["queued_before"], json!(1));
    assert_eq!(env["data"]["queued_after"], json!(0));
    let remote_main = git_ok(&["--git-dir", &remote, "rev-parse", "main"]);
    assert!(
        !remote_main.is_empty(),
        "retry should push the queued operation to the remote"
    );
}

#[test]
fn ops_retry_keeps_backlog_when_remote_is_unreachable() {
    let root = TestDir::new("ops-retry-unreachable");
    queue_operation_behind_broken_remote(root.path());

    let (output, env) = run_loom(root.path(), &["ops", "retry"]);

    assert!(!output.status.success(), "retry unexpectedly succeeded");
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("REMOTE_UNREACHABLE"));
    let pending = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(
        pending["data"]["count"],
        json!(1),
        "failed retry must not drop the queued operation: {pending}"
    );
}

#[test]
fn sync_status_reports_pending_push_backlog_and_synced_recovery() {
    let root = TestDir::new("sync-status-recovery");
    let remote_holder = TestDir::new("sync-status-recovery-remote");
    queue_operation_behind_broken_remote(root.path());

    let pending = run_loom_ok(root.path(), &["sync", "status"]);
    assert_eq!(
        pending["data"]["registry_transport"]["state"],
        json!("PENDING_PUSH")
    );
    assert_eq!(pending["data"]["remote"]["configured"], json!(true));
    assert_eq!(pending["data"]["remote"]["operation_backlog"], json!(1));
    assert_eq!(pending["meta"]["sync_state"], json!("PENDING_PUSH"));

    let remote = init_bare_remote(&remote_holder);
    repoint_remote(root.path(), &remote);
    run_loom_ok(root.path(), &["sync", "replay"]);

    let synced = run_loom_ok(root.path(), &["sync", "status"]);
    assert_eq!(
        synced["data"]["registry_transport"]["state"],
        json!("SYNCED")
    );
    assert_eq!(synced["data"]["remote"]["operation_backlog"], json!(0));
    assert_eq!(synced["meta"]["sync_state"], json!("SYNCED"));
}
