use std::path::Path;
use std::process::Command;

use serde_json::Value;

mod common;

use common::actions::save_skill;
use common::{TestDir, run_loom, write_file, write_skill};

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn git_success(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stderr={} stdout={}",
        args,
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn skill_history_lists_commits_refs_diff_stats_and_operations() {
    let root = TestDir::new("skill-history");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v1");

    write_skill(root.path(), "demo", "# Demo\n\nv2\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v2");
    assert_success(
        &run_loom(root.path(), &["skill", "snapshot", "demo"]).0,
        "snapshot",
    );
    assert_success(
        &run_loom(root.path(), &["skill", "release", "demo", "v1.0.0"]).0,
        "release",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "history", "demo", "--include-diff-stat"],
    );

    assert_success(&output, "history");
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["cmd"], Value::String("skill.history".to_string()));
    assert_eq!(env["data"]["skill"], Value::String("demo".to_string()));
    let items = env["data"]["items"].as_array().expect("history items");
    assert_eq!(items.len(), 2);
    assert!(
        items[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("save(demo)"))
    );
    assert_eq!(
        items[0]["operations"][0]["intent"],
        Value::String("skill.save".to_string())
    );
    let refs = items[0]["refs"].as_array().expect("refs");
    assert!(refs.iter().any(|value| {
        value
            .as_str()
            .is_some_and(|r| r.starts_with("snapshot/demo/"))
    }));
    assert!(refs.iter().any(|value| value == "release/demo/v1.0.0"));
    assert_eq!(items[0]["diff_stat"]["files_changed"], Value::from(1));
}

#[test]
fn skill_history_is_read_only_in_empty_directory() {
    let root = TestDir::new("skill-history-empty");

    let (output, env) = run_loom(root.path(), &["skill", "history", "demo"]);

    assert!(!output.status.success(), "history unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert!(!root.path().join(".git").exists());
    assert!(!root.path().join("state/events").exists());
    assert!(!root.path().join("state/registry").exists());
}

#[test]
fn skill_history_limit_caps_results() {
    let root = TestDir::new("skill-history-limit");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v1");
    write_skill(root.path(), "demo", "# Demo\n\nv2\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v2");

    let (output, env) = run_loom(root.path(), &["skill", "history", "demo", "--limit", "1"]);

    assert_success(&output, "history limit");
    assert_eq!(
        env["data"]["items"].as_array().map(Vec::len),
        Some(1),
        "history limit should cap results"
    );
}

#[test]
fn skill_history_rejects_unsafe_revision_arguments() {
    let root = TestDir::new("skill-history-unsafe-rev");

    let (output, env) = run_loom(root.path(), &["skill", "history", "demo", "--to=--all"]);

    assert!(!output.status.success(), "history unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("--to must be a safe Git revision"))
    );
    assert!(!root.path().join(".git").exists());
    assert!(!root.path().join("state/events").exists());
}

#[test]
fn skill_history_links_operations_by_effect_commit() {
    let root = TestDir::new("skill-history-effect-commit");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v1");
    write_skill(root.path(), "demo", "# Demo\n\nv2\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v2");

    let (rollback_output, rollback_env) =
        run_loom(root.path(), &["skill", "rollback", "demo", "--steps", "1"]);
    assert_success(&rollback_output, "rollback");
    let rollback_commit = rollback_env["data"]["commit"]
        .as_str()
        .expect("rollback commit")
        .to_string();

    let (output, env) = run_loom(root.path(), &["skill", "history", "demo", "--limit", "1"]);

    assert_success(&output, "history");
    let items = env["data"]["items"].as_array().expect("history items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["commit"], Value::String(rollback_commit));
    assert!(
        items[0]["operations"]
            .as_array()
            .is_some_and(|operations| operations.iter().any(
                |operation| operation["intent"] == Value::String("skill.rollback".to_string())
            )),
        "rollback operation should be attached through effects.commit"
    );
}

#[test]
fn skill_history_warns_on_malformed_operation_records() {
    let root = TestDir::new("skill-history-malformed-op");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v1");

    write_skill(root.path(), "demo", "# Demo\n\nv2\n");
    write_file(
        &root.path().join("state/registry/ops/operations.jsonl"),
        "not-json\n",
    );
    git_success(
        root.path(),
        &[
            "add",
            "--",
            "skills/demo",
            "state/registry/ops/operations.jsonl",
        ],
    );
    git_success(root.path(), &["commit", "-m", "save(demo): malformed op"]);

    let (output, env) = run_loom(root.path(), &["skill", "history", "demo"]);

    assert_success(&output, "history malformed op");
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("skipped malformed registry operation"))))
    );
    assert_eq!(
        env["data"]["items"].as_array().map(Vec::len),
        Some(2),
        "malformed operation should not hide valid history"
    );
}
