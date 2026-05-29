mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::actions::save_skill;
use common::{TestDir, run_loom, write_file, write_skill};
use serde_json::Value;

fn run_watch_git(root: &Path, args: &[&str]) -> String {
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

fn read_watch_operations(root: &Path) -> String {
    fs::read_to_string(root.join("state/registry/ops/operations.jsonl")).unwrap_or_default()
}

#[test]
fn skill_watch_dry_run_reports_changed_files_without_commit() {
    let root = TestDir::new("skill-watch-dry-run");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (save_output, _) = save_skill(root.path(), "demo");
    assert!(save_output.status.success(), "initial save failed");
    let head_before = run_watch_git(root.path(), &["rev-parse", "HEAD"]);

    write_skill(root.path(), "demo", "# demo\n\nv2\n");
    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "demo",
            "--once",
            "--dry-run",
            "--debounce-ms",
            "0",
        ],
    );

    assert!(
        output.status.success(),
        "watch dry-run failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(env["data"]["noop"], Value::Bool(false));
    let changed = env["data"]["changed_skills"]
        .as_array()
        .expect("changed skills");
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["skill"], Value::String("demo".to_string()));
    assert_eq!(
        changed[0]["paths"],
        Value::Array(vec![Value::String("skills/demo/SKILL.md".to_string())])
    );
    assert_eq!(changed[0]["would_commit"], Value::Bool(true));
    assert_eq!(
        run_watch_git(root.path(), &["rev-parse", "HEAD"]),
        head_before
    );
    assert!(!read_watch_operations(root.path()).contains(r#""intent":"skill.autosave""#));
}

#[test]
fn skill_watch_dry_run_without_git_repo_reports_files_without_initializing_repo() {
    let root = TestDir::new("skill-watch-dry-run-no-git");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "demo",
            "--once",
            "--dry-run",
            "--debounce-ms",
            "0",
        ],
    );

    assert!(
        output.status.success(),
        "watch dry-run failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    let changed = env["data"]["changed_skills"]
        .as_array()
        .expect("changed skills");
    assert_eq!(changed.len(), 1);
    assert_eq!(
        changed[0]["paths"],
        Value::Array(vec![Value::String("skills/demo/SKILL.md".to_string())])
    );
    assert!(
        !root.path().join(".git").exists(),
        "dry-run should not initialize a git repository"
    );
    assert!(!read_watch_operations(root.path()).contains(r#""intent":"skill.autosave""#));
}

#[test]
fn skill_watch_once_commits_changed_source_and_records_autosave() {
    let root = TestDir::new("skill-watch-autosave");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (save_output, _) = save_skill(root.path(), "demo");
    assert!(save_output.status.success(), "initial save failed");

    write_skill(root.path(), "demo", "# demo\n\nv2\n");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "watch", "demo", "--once", "--debounce-ms", "0"],
    );

    assert!(
        output.status.success(),
        "watch failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["noop"], Value::Bool(false));
    let saved = env["data"]["saved_skills"]
        .as_array()
        .expect("saved skills");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["skill"], Value::String("demo".to_string()));
    assert_eq!(
        saved[0]["paths"],
        Value::Array(vec![Value::String("skills/demo/SKILL.md".to_string())])
    );
    assert!(saved[0]["commit"].as_str().is_some());
    assert_eq!(
        run_watch_git(root.path(), &["log", "-1", "--pretty=%s"]),
        "autosave(demo): captured local edits"
    );
    let operations = read_watch_operations(root.path());
    assert!(operations.contains(r#""intent":"skill.autosave""#));
    assert!(operations.contains(r#""skill":"demo""#));
    assert_eq!(
        run_watch_git(
            root.path(),
            &[
                "status",
                "--porcelain",
                "--",
                "skills/demo",
                "state/registry"
            ]
        ),
        ""
    );
}

#[test]
fn skill_watch_ignores_temp_and_local_state_paths() {
    let root = TestDir::new("skill-watch-ignore");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (save_output, _) = save_skill(root.path(), "demo");
    assert!(save_output.status.success(), "initial save failed");

    write_file(&root.path().join("skills/demo/state/cache.txt"), "cache\n");
    write_file(&root.path().join("skills/demo/trash/old.txt"), "trash\n");
    write_file(&root.path().join("skills/demo/backups/old.txt"), "backup\n");
    write_file(&root.path().join("skills/demo/.DS_Store"), "finder\n");
    write_file(&root.path().join("skills/demo/draft.tmp"), "tmp\n");
    write_file(&root.path().join("skills/demo/draft.swp"), "swap\n");
    write_file(&root.path().join("skills/demo/draft~"), "backup\n");

    let (ignored_output, ignored_env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "--once",
            "--dry-run",
            "--debounce-ms",
            "0",
        ],
    );
    assert!(
        ignored_output.status.success(),
        "ignored-only dry-run failed"
    );
    assert_eq!(ignored_env["data"]["noop"], Value::Bool(true));
    assert_eq!(
        ignored_env["data"]["changed_skills"],
        Value::Array(Vec::new())
    );

    write_skill(root.path(), "demo", "# demo\n\nv2\n");
    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "--once",
            "--dry-run",
            "--debounce-ms",
            "0",
        ],
    );
    assert!(output.status.success(), "dry-run failed after source edit");
    let changed = env["data"]["changed_skills"]
        .as_array()
        .expect("changed skills");
    assert_eq!(changed.len(), 1);
    assert_eq!(
        changed[0]["paths"],
        Value::Array(vec![Value::String("skills/demo/SKILL.md".to_string())])
    );
}

#[test]
fn skill_watch_refuses_batches_over_max_batch() {
    let root = TestDir::new("skill-watch-max-batch");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (save_output, _) = save_skill(root.path(), "demo");
    assert!(save_output.status.success(), "initial save failed");
    let head_before = run_watch_git(root.path(), &["rev-parse", "HEAD"]);

    write_file(&root.path().join("skills/demo/a.md"), "a\n");
    write_file(&root.path().join("skills/demo/b.md"), "b\n");
    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "demo",
            "--once",
            "--debounce-ms",
            "0",
            "--max-batch",
            "1",
        ],
    );

    assert!(!output.status.success(), "watch unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("DEPENDENCY_CONFLICT".to_string())
    );
    assert!(
        env["error"]["message"]
            .as_str()
            .expect("error message")
            .contains("exceeding --max-batch")
    );
    assert_eq!(
        run_watch_git(root.path(), &["rev-parse", "HEAD"]),
        head_before
    );
    assert!(!read_watch_operations(root.path()).contains(r#""intent":"skill.autosave""#));
}
