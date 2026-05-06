mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use common::actions::target_add;
use common::{TestDir, run_loom, run_loom_with_env, write_file};

fn git_head(root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("read git head");
    assert!(
        output.status.success(),
        "rev-parse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn monitor_observed_imports_updates_then_noops() {
    let root = TestDir::new("monitor-observed");
    let observed = root.path().join("observed-skills");

    write_file(&observed.join("alpha/SKILL.md"), "# alpha\n");
    write_file(&observed.join("alpha/config.txt"), "one\n");

    let (target_output, target_env) = target_add(root.path(), "claude", &observed, "observed");
    assert!(
        target_output.status.success(),
        "target add failed: {}",
        String::from_utf8_lossy(&target_output.stderr)
    );
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();

    let (first_output, first_env) = run_loom(
        root.path(),
        &[
            "skill",
            "monitor-observed",
            "--once",
            "--target",
            &target_id,
        ],
    );
    assert!(
        first_output.status.success(),
        "first monitor failed: stderr={} stdout={}",
        String::from_utf8_lossy(&first_output.stderr),
        String::from_utf8_lossy(&first_output.stdout)
    );
    assert_eq!(
        first_env["cmd"],
        Value::String("skill.monitor_observed".into())
    );
    assert_eq!(first_env["data"]["cycles"], Value::from(1));
    assert_eq!(
        first_env["data"]["last_cycle"]["imported_count"],
        Value::from(1)
    );
    assert_eq!(
        first_env["data"]["last_cycle"]["updated_count"],
        Value::from(0)
    );
    assert_eq!(first_env["data"]["last_cycle"]["noop"], Value::Bool(false));
    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/SKILL.md")).expect("read alpha"),
        "# alpha\n"
    );

    let first_head = git_head(root.path());
    write_file(&observed.join("alpha/SKILL.md"), "# alpha\n\nupdated\n");
    write_file(&observed.join("alpha/config.txt"), "two\n");

    let (second_output, second_env) = run_loom(
        root.path(),
        &[
            "skill",
            "monitor-observed",
            "--once",
            "--target",
            &target_id,
        ],
    );
    assert!(
        second_output.status.success(),
        "second monitor failed: stderr={} stdout={}",
        String::from_utf8_lossy(&second_output.stderr),
        String::from_utf8_lossy(&second_output.stdout)
    );
    assert_eq!(
        second_env["data"]["last_cycle"]["imported_count"],
        Value::from(0)
    );
    assert_eq!(
        second_env["data"]["last_cycle"]["updated_count"],
        Value::from(1)
    );
    assert_ne!(git_head(root.path()), first_head);
    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/SKILL.md")).expect("read updated alpha"),
        "# alpha\n\nupdated\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/config.txt"))
            .expect("read updated config"),
        "two\n"
    );

    let second_head = git_head(root.path());
    let (third_output, third_env) = run_loom(
        root.path(),
        &[
            "skill",
            "monitor-observed",
            "--once",
            "--target",
            &target_id,
        ],
    );
    assert!(
        third_output.status.success(),
        "third monitor failed: stderr={} stdout={}",
        String::from_utf8_lossy(&third_output.stderr),
        String::from_utf8_lossy(&third_output.stdout)
    );
    assert_eq!(third_env["data"]["last_cycle"]["noop"], Value::Bool(true));
    assert_eq!(
        third_env["data"]["last_cycle"]["unchanged_count"],
        Value::from(1)
    );
    assert_eq!(git_head(root.path()), second_head);
}

#[test]
fn monitor_observed_rolls_back_update_after_operation_failure() {
    let root = TestDir::new("monitor-observed-op-failure");
    let observed = root.path().join("observed-skills");

    write_file(&observed.join("alpha/SKILL.md"), "# alpha\n");
    write_file(&observed.join("alpha/config.txt"), "one\n");

    let (target_output, target_env) = target_add(root.path(), "claude", &observed, "observed");
    assert!(
        target_output.status.success(),
        "target add failed: {}",
        String::from_utf8_lossy(&target_output.stderr)
    );
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();

    let (first_output, _) = run_loom(
        root.path(),
        &[
            "skill",
            "monitor-observed",
            "--once",
            "--target",
            &target_id,
        ],
    );
    assert!(
        first_output.status.success(),
        "initial monitor failed: stderr={} stdout={}",
        String::from_utf8_lossy(&first_output.stderr),
        String::from_utf8_lossy(&first_output.stdout)
    );
    let head_before = git_head(root.path());

    write_file(&observed.join("alpha/SKILL.md"), "# alpha\n\nupdated\n");
    write_file(&observed.join("alpha/config.txt"), "two\n");

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_append")],
        &[
            "skill",
            "monitor-observed",
            "--once",
            "--target",
            &target_id,
        ],
    );

    assert!(
        !output.status.success(),
        "faulted monitor unexpectedly succeeded"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(git_head(root.path()), head_before);
    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/SKILL.md")).expect("read alpha"),
        "# alpha\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("skills/alpha/config.txt")).expect("read config"),
        "one\n"
    );
}
