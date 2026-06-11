mod common;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use common::actions::save_skill;
use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};
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

fn rollback_error_steps(env: &Value) -> Vec<String> {
    env["error"]["details"]["rollback_errors"]
        .as_array()
        .expect("rollback errors array")
        .iter()
        .filter_map(|error| error["step"].as_str().map(ToString::to_string))
        .collect()
}

fn write_live_workspace_lock(root: &Path) {
    let locks = root.join("state/locks");
    fs::create_dir_all(&locks).expect("create locks dir");
    fs::write(locks.join("workspace.lock"), "busy\n").expect("write workspace lock");
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
fn skill_watch_once_preserves_unrelated_staged_changes() {
    let root = TestDir::new("skill-watch-preserve-staged");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    let (save_output, _) = save_skill(root.path(), "demo");
    assert!(save_output.status.success(), "initial save failed");

    write_file(&root.path().join("README.md"), "staged but unrelated\n");
    run_watch_git(root.path(), &["add", "README.md"]);
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
    assert_eq!(
        run_watch_git(root.path(), &["log", "-1", "--pretty=%s"]),
        "autosave(demo): captured local edits"
    );
    let committed_paths = run_watch_git(root.path(), &["show", "--name-only", "--pretty=", "HEAD"]);
    assert!(committed_paths.contains("skills/demo/SKILL.md"));
    assert!(committed_paths.contains("state/registry/ops/operations.jsonl"));
    assert!(
        !committed_paths.contains("README.md"),
        "autosave commit included unrelated staged path: {committed_paths}"
    );
    assert_eq!(
        run_watch_git(root.path(), &["status", "--porcelain", "--", "README.md"]),
        "A  README.md"
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

#[test]
fn skill_watch_long_running_skips_lock_busy_cycles() {
    let root = TestDir::new("skill-watch-lock-busy-continue");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());
    write_skill(root.path(), "demo", "# demo\n\nv2\n");
    write_live_workspace_lock(root.path());

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "demo",
            "--debounce-ms",
            "1",
            "--max-cycles",
            "2",
        ],
    );

    assert!(
        output.status.success(),
        "watch exited on LOCK_BUSY: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["data"]["cycles"], Value::from(2));
    assert_eq!(env["data"]["skipped_total"], Value::from(2));
    assert_eq!(
        env["data"]["last_cycle"]["skipped"][0]["error"]["code"],
        Value::String("LOCK_BUSY".to_string())
    );
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning.as_str().unwrap_or_default().contains("LOCK_BUSY"))
    );
}

#[test]
fn skill_watch_long_running_skips_capture_conflict_cycles() {
    let root = TestDir::new("skill-watch-capture-conflict-continue");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());
    write_skill(root.path(), "demo", "# demo\n\nv2\n");

    let root_path = root.path().to_path_buf();
    let mutator = thread::spawn(move || {
        for index in 0..80 {
            thread::sleep(Duration::from_millis(20));
            write_file(
                &root_path.join(format!("skills/demo/race-{index}.md")),
                &format!("{index}\n"),
            );
        }
    });
    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "watch",
            "demo",
            "--debounce-ms",
            "100",
            "--max-cycles",
            "2",
        ],
    );
    mutator.join().expect("mutator thread");

    assert!(
        output.status.success(),
        "watch exited on CAPTURE_CONFLICT: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        env["data"]["skipped_total"].as_u64().unwrap_or(0) >= 1,
        "expected at least one skipped conflict cycle: {env}"
    );
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap_or_default()
                .contains("CAPTURE_CONFLICT")),
        "missing capture conflict warning: {env}"
    );
}

#[cfg(unix)]
#[test]
fn skill_watch_long_running_preserves_saved_results_when_one_skill_fails() {
    let root = TestDir::new("skill-watch-partial-failure");
    write_skill(root.path(), "alpha", "# alpha\n\nv1\n");
    write_skill(root.path(), "beta", "# beta\n\nv1\n");
    assert!(save_skill(root.path(), "alpha").0.status.success());
    assert!(save_skill(root.path(), "beta").0.status.success());

    let hook = root.path().join(".git/hooks/pre-commit");
    fs::create_dir_all(hook.parent().unwrap()).expect("create hooks dir");
    fs::write(
        &hook,
        "#!/bin/sh\nif git diff --cached --name-only | grep -q '^skills/beta/'; then\n  echo beta blocked >&2\n  exit 1\nfi\nexit 0\n",
    )
    .expect("write pre-commit hook");
    #[allow(clippy::permissions_set_readonly_false)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook).expect("hook metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook, perms).expect("set hook executable");
    }

    write_skill(root.path(), "alpha", "# alpha\n\nv2\n");
    write_skill(root.path(), "beta", "# beta\n\nv2\n");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "watch", "--debounce-ms", "1", "--max-cycles", "1"],
    );

    assert!(
        output.status.success(),
        "watch should keep saved results after one skill fails: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["data"]["saved_total"], Value::from(1));
    assert_eq!(env["data"]["skipped_total"], Value::from(1));
    assert_eq!(
        env["data"]["last_cycle"]["saved_skills"][0]["skill"],
        Value::String("alpha".to_string())
    );
    assert_eq!(
        env["data"]["last_cycle"]["skipped"][0]["skill"],
        Value::String("beta".to_string())
    );
}

#[test]
fn skill_watch_once_reports_audit_restore_rollback_errors() {
    let root = TestDir::new("skill-watch-rollback-errors");
    write_skill(root.path(), "demo", "# demo\n\nv1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());
    write_skill(root.path(), "demo", "# demo\n\nv2\n");

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
            ("LOOM_ROLLBACK_FAULT_INJECT", "restore_registry_audit_state"),
        ],
        &["skill", "watch", "demo", "--once", "--debounce-ms", "0"],
    );

    assert!(
        !output.status.success(),
        "faulted watch unexpectedly succeeded"
    );
    assert!(
        rollback_error_steps(&env).contains(&"restore_registry_audit_state".to_string()),
        "missing rollback error details: {env}"
    );
}
