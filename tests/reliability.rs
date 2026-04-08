use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};
use std::thread;

use serde_json::Value;
use uuid::Uuid;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("loom-{}-{}", prefix, Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn loom_bin() -> &'static str {
    env!("CARGO_BIN_EXE_loom")
}

fn run_loom_with_env(root: &Path, envs: &[(&str, &str)], args: &[&str]) -> (Output, Value) {
    let mut cmd = Command::new(loom_bin());
    cmd.arg("--json").arg("--root").arg(root).args(args);
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

fn run_loom_ok(root: &Path, args: &[&str]) -> Value {
    let (output, env) = run_loom_with_env(root, &[], args);
    assert!(
        output.status.success(),
        "loom failed: status={:?} stderr={} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    env
}

fn run_git<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .args(args)
        .output()
        .expect("run git")
}

fn git_ok<I, S>(args: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(args);
    assert!(
        output.status.success(),
        "git failed: status={:?} stderr={} stdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stdout).expect("git stdout utf8")
}

fn git_ok_in(repo: &Path, args: &[&str]) -> String {
    let mut full = vec!["-C", repo.to_str().unwrap()];
    full.extend_from_slice(args);
    git_ok(full)
}

fn git_set_identity(repo: &Path, name: &str, email: &str) {
    git_ok_in(repo, &["config", "--local", "user.name", name]);
    git_ok_in(repo, &["config", "--local", "user.email", email]);
}

fn git_branch_exists(repo: &Path, branch: &str) -> bool {
    run_git([
        "-C",
        repo.to_str().unwrap(),
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ])
    .status
    .success()
}

fn git_branch_parent_count(repo: &Path, branch: &str) -> usize {
    let parents = git_ok_in(repo, &["rev-list", "--parents", "-n", "1", branch]);
    parents.split_whitespace().count().saturating_sub(1)
}

fn git_branch_path_count(repo: &Path, branch: &str, prefix: &str) -> usize {
    git_ok_in(repo, &["ls-tree", "-r", "--name-only", branch])
        .lines()
        .filter(|line| line.starts_with(prefix))
        .count()
}

fn history_segment_count(root: &Path) -> usize {
    let dir = root.join("state/pending_ops_history");
    if !dir.exists() {
        return 0;
    }
    fs::read_dir(dir)
        .expect("read history dir")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|ty| ty.is_file()).unwrap_or(false))
        .count()
}

fn make_skill_source(root: &Path, name: &str) -> PathBuf {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(skill_dir.join("SKILL.md"), format!("# {}\n", name)).expect("write SKILL.md");
    skill_dir
}

fn make_skill_source_with_contents(root: &Path, name: &str, contents: &str) -> PathBuf {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(skill_dir.join("SKILL.md"), contents).expect("write SKILL.md");
    skill_dir
}

fn replace_history_branch(
    repo_target: &Path,
    branch: &str,
    files: &[(String, String)],
    message: &str,
) {
    let clone = TestDir::new(&format!("history-branch-{}", branch));
    git_ok([
        "clone",
        repo_target.to_str().unwrap(),
        clone.path().to_str().unwrap(),
    ]);
    git_set_identity(clone.path(), "loom", "loom@local");

    let remote_branch = format!("refs/heads/{branch}");
    let remote_exists = run_git([
        "-C",
        clone.path().to_str().unwrap(),
        "ls-remote",
        "--exit-code",
        "origin",
        &remote_branch,
    ])
    .status
    .success();

    if remote_exists {
        git_ok_in(
            clone.path(),
            &["checkout", "-B", branch, &format!("origin/{branch}")],
        );
    } else {
        git_ok_in(clone.path(), &["checkout", "--orphan", branch]);
    }

    clear_checkout_contents(clone.path());
    for (path, contents) in files {
        let file = clone.path().join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).expect("create history parent");
        }
        fs::write(file, contents).expect("write history file");
    }

    git_ok_in(clone.path(), &["add", "-A"]);
    git_ok_in(clone.path(), &["commit", "-m", message]);
    git_ok_in(
        clone.path(),
        &["push", "--force", "origin", &format!("HEAD:{branch}")],
    );
}

fn clear_checkout_contents(root: &Path) {
    for entry in fs::read_dir(root).expect("read checkout dir") {
        let entry = entry.expect("read checkout entry");
        let path = entry.path();
        if entry.file_name() == ".git" {
            continue;
        }
        if entry.file_type().expect("file type").is_dir() {
            fs::remove_dir_all(path).expect("remove checkout dir");
        } else {
            fs::remove_file(path).expect("remove checkout file");
        }
    }
}

fn history_event_timestamp(index: usize) -> String {
    let minutes = index / 60;
    let seconds = index % 60;
    format!("2026-04-08T00:{minutes:02}:{seconds:02}Z")
}

fn history_event_line(scope: &str, index: usize) -> String {
    let ts = history_event_timestamp(index);
    format!(
        "{{\"event\":\"queued\",\"event_id\":\"{scope}-event-{index:03}\",\"at\":\"{ts}\",\"op\":{{\"op_id\":\"{scope}-op-{index:03}\",\"request_id\":\"{scope}-req-{index:03}\",\"command\":\"snapshot\",\"created_at\":\"{ts}\",\"details\":{{\"skill\":\"demo\",\"ordinal\":{index}}}}}}}\n"
    )
}

#[test]
fn workspace_status_is_read_only_in_empty_dir() {
    let root = TestDir::new("status");

    let env = run_loom_ok(root.path(), &["workspace", "status"]);

    assert_eq!(env["ok"], true);
    assert!(!root.path().join(".git").exists());
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("skills").exists());
}

#[test]
fn remote_set_initializes_repo_and_local_identity() {
    let root = TestDir::new("remote-set");
    let remote = root.path().join("remote.git");

    let env = run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );

    assert_eq!(env["ok"], true);
    assert!(root.path().join(".git").exists());
    assert_eq!(
        git_ok([
            "-C",
            root.path().to_str().unwrap(),
            "config",
            "--local",
            "user.name"
        ])
        .trim(),
        "loom"
    );
    assert_eq!(
        git_ok([
            "-C",
            root.path().to_str().unwrap(),
            "config",
            "--local",
            "user.email"
        ])
        .trim(),
        "loom@local"
    );
}

#[test]
fn ops_list_skips_malformed_pending_lines() {
    let root = TestDir::new("ops-list");
    let state_dir = root.path().join("state");
    fs::create_dir_all(&state_dir).expect("create state dir");
    fs::write(
        state_dir.join("pending_ops.jsonl"),
        concat!(
            "{\"request_id\":\"r1\",\"command\":\"save\",\"created_at\":\"2026-04-08T00:00:00Z\",\"details\":{\"skill\":\"demo\"}}\n",
            "not-json\n"
        ),
    )
    .expect("write pending ops");

    let env = run_loom_ok(root.path(), &["ops", "list"]);

    assert_eq!(env["data"]["count"], 1);
    assert_eq!(env["data"]["ops"].as_array().unwrap().len(), 1);
    assert_eq!(env["meta"]["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn queued_ops_replay_after_remote_repair() {
    let root = TestDir::new("replay");
    let source = make_skill_source(root.path(), "source-demo");
    let bad_remote = root.path().join("missing-remote.git");

    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", bad_remote.to_str().unwrap()],
    );

    let add_env = run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    assert_eq!(
        add_env["meta"]["sync_state"].as_str().unwrap(),
        "PENDING_PUSH"
    );

    let pending_before = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending_before["data"]["count"], 1);

    let remote_root = TestDir::new("replay-remote");
    let remote = remote_root.path().join("origin.git");
    git_ok(["init", "--bare", remote.to_str().unwrap()]);

    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    let replay = run_loom_ok(root.path(), &["sync", "push"]);
    assert_eq!(replay["data"]["result"], "pushed");

    let pending_after = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending_after["data"]["count"], 0);
    assert!(
        !git_ok(["--git-dir", remote.to_str().unwrap(), "rev-parse", "main"])
            .trim()
            .is_empty()
    );
}

#[test]
fn snapshot_pushes_annotated_tag_to_remote() {
    let root = TestDir::new("snapshot");
    let remote_root = TestDir::new("snapshot-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(root.path(), "source-snapshot");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    let snapshot = run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    let tag = snapshot["data"]["tag"].as_str().unwrap();

    let refs = git_ok([
        "--git-dir",
        remote.to_str().unwrap(),
        "for-each-ref",
        "refs/tags",
        "--format=%(objecttype) %(refname:strip=2)",
    ]);
    assert!(refs.lines().any(|line| line == format!("tag {}", tag)));

    let pending = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending["data"]["count"], 0);
}

#[test]
fn sync_pull_fast_forwards_from_remote() {
    let root = TestDir::new("pull-ok");
    let remote_root = TestDir::new("pull-ok-remote");
    let peer_root = TestDir::new("pull-ok-peer");
    let remote = remote_root.path().join("origin.git");
    let source =
        make_skill_source_with_contents(root.path(), "source-pull", "# demo\nvalue=base\n");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    git_ok([
        "clone",
        "--branch",
        "main",
        remote.to_str().unwrap(),
        peer_root.path().to_str().unwrap(),
    ]);
    git_set_identity(peer_root.path(), "peer", "peer@example.com");
    fs::write(
        peer_root.path().join("skills/demo/SKILL.md"),
        "# demo\nvalue=remote\n",
    )
    .expect("write peer skill");
    git_ok_in(peer_root.path(), &["add", "--", "skills/demo/SKILL.md"]);
    git_ok_in(peer_root.path(), &["commit", "-m", "remote update"]);
    git_ok_in(peer_root.path(), &["push", "origin", "HEAD:main"]);

    let env = run_loom_ok(root.path(), &["sync", "pull"]);
    assert_eq!(env["data"]["result"], "pulled");
    assert_eq!(env["data"]["replay"], "no_pending_ops");
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read pulled file"),
        "# demo\nvalue=remote\n"
    );
}

#[test]
fn sync_pull_conflict_aborts_rebase_state() {
    let root = TestDir::new("pull-conflict");
    let remote_root = TestDir::new("pull-conflict-remote");
    let peer_root = TestDir::new("pull-conflict-peer");
    let remote = remote_root.path().join("origin.git");
    let source =
        make_skill_source_with_contents(root.path(), "source-conflict", "# demo\nvalue=base\n");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    git_ok([
        "clone",
        "--branch",
        "main",
        remote.to_str().unwrap(),
        peer_root.path().to_str().unwrap(),
    ]);
    git_set_identity(peer_root.path(), "peer", "peer@example.com");
    fs::write(
        peer_root.path().join("skills/demo/SKILL.md"),
        "# demo\nvalue=remote\n",
    )
    .expect("write peer conflict");
    git_ok_in(peer_root.path(), &["add", "--", "skills/demo/SKILL.md"]);
    git_ok_in(peer_root.path(), &["commit", "-m", "remote conflict"]);
    git_ok_in(peer_root.path(), &["push", "origin", "HEAD:main"]);

    fs::write(
        root.path().join("skills/demo/SKILL.md"),
        "# demo\nvalue=local\n",
    )
    .expect("write local conflict");
    git_ok_in(root.path(), &["add", "--", "skills/demo/SKILL.md"]);
    git_ok_in(root.path(), &["commit", "-m", "local conflict"]);

    let (output, env) = run_loom_with_env(root.path(), &[], &["sync", "pull"]);
    assert!(!output.status.success(), "expected sync pull to fail");
    assert_eq!(env["error"]["code"], "REPLAY_CONFLICT");
    assert!(!root.path().join(".git/rebase-merge").exists());
    assert!(!root.path().join(".git/rebase-apply").exists());
}

#[test]
fn stale_skill_lock_is_reaped_automatically() {
    let root = TestDir::new("stale-lock");
    let source = make_skill_source(root.path(), "source-stale");

    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    let lock_path = root.path().join("state/locks/skill-demo.lock");
    fs::create_dir_all(lock_path.parent().unwrap()).expect("create lock dir");
    fs::write(
        &lock_path,
        "{\"pid\":1,\"created_at\":\"2000-01-01T00:00:00Z\"}\n",
    )
    .expect("write stale lock");

    let env = run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    assert_eq!(env["ok"], true);
    assert!(!lock_path.exists());
}

#[test]
fn ops_journal_compacts_into_snapshot_and_history() {
    let root = TestDir::new("ops-compact");
    let source = make_skill_source(root.path(), "source-compact");

    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    for _ in 0..20 {
        run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    }

    let purge = run_loom_ok(root.path(), &["ops", "purge"]);
    assert!(purge["data"]["purged"].as_u64().unwrap() >= 1);

    let list = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(list["data"]["count"], 0);
    assert_eq!(list["data"]["journal_events"], 0);
    assert!(list["data"]["history_events"].as_u64().unwrap() >= 16);
    assert!(root.path().join("state/pending_ops_snapshot.json").exists());
    assert!(history_segment_count(root.path()) >= 1);
    assert_eq!(
        fs::read_to_string(root.path().join("state/pending_ops.jsonl"))
            .expect("read compacted journal"),
        ""
    );

    for _ in 0..20 {
        run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    }
    run_loom_ok(root.path(), &["ops", "purge"]);
    assert!(history_segment_count(root.path()) >= 2);
}

#[test]
fn ops_compaction_mirrors_history_into_local_git_branch() {
    let root = TestDir::new("history-local");
    let remote_root = TestDir::new("history-local-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(root.path(), "source-history-local");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    run_loom_ok(root.path(), &["ops", "list"]);
    git_ok_in(root.path(), &["remote", "remove", "origin"]);

    for _ in 0..16 {
        run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    }

    assert!(git_branch_exists(root.path(), "loom-history"));
    let files = git_ok_in(
        root.path(),
        &["ls-tree", "-r", "--name-only", "loom-history"],
    );
    assert!(
        files
            .lines()
            .any(|line| line == "pending_ops_snapshot.json")
    );
    let segment_path = files
        .lines()
        .find(|line| line.starts_with("pending_ops_history/"))
        .expect("history segment in loom-history branch");
    let snapshot_raw = git_ok_in(
        root.path(),
        &["show", "loom-history:pending_ops_snapshot.json"],
    );
    let snapshot: Value = serde_json::from_str(&snapshot_raw).expect("parse history snapshot");
    assert!(snapshot["history_events"].as_u64().unwrap() >= 16);
    let segment_raw = git_ok_in(
        root.path(),
        &["show", &format!("loom-history:{segment_path}")],
    );
    assert!(segment_raw.lines().count() >= 16);
}

#[test]
fn sync_push_propagates_history_branch_to_remote() {
    let root = TestDir::new("history-push");
    let remote_root = TestDir::new("history-push-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(root.path(), "source-history-push");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    git_ok_in(root.path(), &["remote", "remove", "origin"]);

    for _ in 0..16 {
        run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    }

    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    let push = run_loom_ok(root.path(), &["sync", "push"]);
    assert_eq!(push["data"]["result"], "pushed");
    assert!(
        run_git([
            "--git-dir",
            remote.to_str().unwrap(),
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/loom-history",
        ])
        .status
        .success()
    );
    let files = git_ok([
        "--git-dir",
        remote.to_str().unwrap(),
        "ls-tree",
        "-r",
        "--name-only",
        "loom-history",
    ]);
    assert!(
        files
            .lines()
            .any(|line| line == "pending_ops_snapshot.json")
    );
    assert!(
        files
            .lines()
            .any(|line| line.starts_with("pending_ops_history/"))
    );

    let pending = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending["data"]["count"], 0);
}

#[test]
fn sync_pull_creates_local_history_branch_from_remote() {
    let producer = TestDir::new("history-pull-producer");
    let consumer = TestDir::new("history-pull-consumer");
    let remote_root = TestDir::new("history-pull-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(producer.path(), "source-history-pull");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        producer.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    git_ok_in(producer.path(), &["remote", "remove", "origin"]);

    for _ in 0..16 {
        run_loom_ok(producer.path(), &["skill", "snapshot", "demo"]);
    }

    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(producer.path(), &["sync", "push"]);

    git_ok([
        "clone",
        "--branch",
        "main",
        remote.to_str().unwrap(),
        consumer.path().to_str().unwrap(),
    ]);

    assert!(!git_branch_exists(consumer.path(), "loom-history"));
    let pull = run_loom_ok(consumer.path(), &["sync", "pull"]);
    assert_eq!(pull["data"]["result"], "pulled");
    assert!(
        pull["meta"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap_or_default()
                .contains("created local loom-history"))
    );
    assert!(git_branch_exists(consumer.path(), "loom-history"));
    let files = git_ok_in(
        consumer.path(),
        &["ls-tree", "-r", "--name-only", "loom-history"],
    );
    assert!(
        files
            .lines()
            .any(|line| line == "pending_ops_snapshot.json")
    );
    assert!(
        files
            .lines()
            .any(|line| line.starts_with("pending_ops_history/"))
    );
}

#[test]
fn sync_push_reconciles_divergent_history_branch_before_push() {
    let producer = TestDir::new("history-reconcile-push-producer");
    let consumer = TestDir::new("history-reconcile-push-consumer");
    let remote_root = TestDir::new("history-reconcile-push-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(producer.path(), "source-history-reconcile-push");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        producer.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    git_ok([
        "clone",
        "--branch",
        "main",
        remote.to_str().unwrap(),
        consumer.path().to_str().unwrap(),
    ]);

    git_ok_in(producer.path(), &["remote", "remove", "origin"]);
    for _ in 0..16 {
        run_loom_ok(producer.path(), &["skill", "snapshot", "demo"]);
    }
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(producer.path(), &["sync", "push"]);

    run_loom_ok(consumer.path(), &["sync", "pull"]);

    git_ok_in(consumer.path(), &["remote", "remove", "origin"]);
    for _ in 0..16 {
        run_loom_ok(consumer.path(), &["skill", "snapshot", "demo"]);
    }

    git_ok_in(producer.path(), &["remote", "remove", "origin"]);
    for _ in 0..16 {
        run_loom_ok(producer.path(), &["skill", "snapshot", "demo"]);
    }
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(producer.path(), &["sync", "push"]);

    run_loom_ok(
        consumer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    let push = run_loom_ok(consumer.path(), &["sync", "push"]);
    assert_eq!(push["data"]["result"], "pushed");

    let pending = run_loom_ok(consumer.path(), &["ops", "list"]);
    assert_eq!(pending["data"]["count"], 0);

    let parents = git_ok([
        "--git-dir",
        remote.to_str().unwrap(),
        "rev-list",
        "--parents",
        "-n",
        "1",
        "loom-history",
    ]);
    assert!(parents.split_whitespace().count() >= 3);

    let files = git_ok([
        "--git-dir",
        remote.to_str().unwrap(),
        "ls-tree",
        "-r",
        "--name-only",
        "loom-history",
    ]);
    assert!(
        files
            .lines()
            .filter(|line| line.starts_with("pending_ops_history/"))
            .count()
            >= 3
    );

    let snapshot_raw = git_ok([
        "--git-dir",
        remote.to_str().unwrap(),
        "show",
        "loom-history:pending_ops_snapshot.json",
    ]);
    let snapshot: Value = serde_json::from_str(&snapshot_raw).expect("parse reconciled snapshot");
    assert!(snapshot["history_events"].as_u64().unwrap() >= 48);
}

#[test]
fn sync_pull_reconciles_divergent_history_branch_without_pending_ops() {
    let producer = TestDir::new("history-reconcile-pull-producer");
    let consumer = TestDir::new("history-reconcile-pull-consumer");
    let remote_root = TestDir::new("history-reconcile-pull-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(producer.path(), "source-history-reconcile-pull");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        producer.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    git_ok([
        "clone",
        "--branch",
        "main",
        remote.to_str().unwrap(),
        consumer.path().to_str().unwrap(),
    ]);

    git_ok_in(producer.path(), &["remote", "remove", "origin"]);
    for _ in 0..16 {
        run_loom_ok(producer.path(), &["skill", "snapshot", "demo"]);
    }
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(producer.path(), &["sync", "push"]);

    run_loom_ok(consumer.path(), &["sync", "pull"]);

    git_ok_in(consumer.path(), &["remote", "remove", "origin"]);
    for _ in 0..16 {
        run_loom_ok(consumer.path(), &["skill", "snapshot", "demo"]);
    }
    run_loom_ok(consumer.path(), &["ops", "purge"]);
    let pending_before = run_loom_ok(consumer.path(), &["ops", "list"]);
    assert_eq!(pending_before["data"]["count"], 0);

    git_ok_in(producer.path(), &["remote", "remove", "origin"]);
    for _ in 0..16 {
        run_loom_ok(producer.path(), &["skill", "snapshot", "demo"]);
    }
    run_loom_ok(
        producer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(producer.path(), &["sync", "push"]);

    run_loom_ok(
        consumer.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    let pull = run_loom_ok(consumer.path(), &["sync", "pull"]);
    assert_eq!(pull["data"]["result"], "pulled");
    assert_eq!(pull["data"]["replay"], "no_pending_ops");
    assert!(
        pull["meta"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap_or_default()
                .contains("reconciled divergent loom-history branches"))
    );

    assert!(git_branch_parent_count(consumer.path(), "loom-history") >= 2);
    let files = git_ok_in(
        consumer.path(),
        &["ls-tree", "-r", "--name-only", "loom-history"],
    );
    assert!(
        files
            .lines()
            .filter(|line| line.starts_with("pending_ops_history/"))
            .count()
            >= 4
    );
    let snapshot_raw = git_ok_in(
        consumer.path(),
        &["show", "loom-history:pending_ops_snapshot.json"],
    );
    let snapshot: Value = serde_json::from_str(&snapshot_raw).expect("parse pulled snapshot");
    assert!(snapshot["history_events"].as_u64().unwrap() >= 64);
}

fn assert_compaction_fault_recovers(injection_point: &str) {
    let root = TestDir::new(&format!("fault-{}", injection_point));
    let remote_root = TestDir::new(&format!("fault-remote-{}", injection_point));
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(root.path(), "source-fault");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    let baseline = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(baseline["data"]["count"], 0);
    git_ok_in(root.path(), &["remote", "remove", "origin"]);

    for _ in 0..15 {
        run_loom_ok(root.path(), &["skill", "snapshot", "demo"]);
    }

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", injection_point)],
        &["skill", "snapshot", "demo"],
    );
    assert!(!output.status.success(), "expected injected failure");
    assert_eq!(env["error"]["code"], "QUEUE_BLOCKED");

    let pending_before = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending_before["data"]["count"], 16);

    let purge = run_loom_ok(root.path(), &["ops", "purge"]);
    assert_eq!(purge["data"]["purged"], 16);

    let pending_after = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending_after["data"]["count"], 0);
    assert!(pending_after["data"]["history_events"].as_u64().unwrap() >= 16);
    assert!(root.path().join("state/pending_ops_snapshot.json").exists());
    assert!(history_segment_count(root.path()) >= 1);
}

#[test]
fn compaction_recovers_after_history_fault_injection() {
    assert_compaction_fault_recovers("ops_compact_after_history");
}

#[test]
fn compaction_recovers_after_snapshot_fault_injection() {
    assert_compaction_fault_recovers("ops_compact_after_snapshot");
}

#[test]
fn concurrent_snapshots_keep_pending_journal_consistent() {
    let root = TestDir::new("concurrent-snapshot");
    let source = make_skill_source(root.path(), "source-concurrent");

    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    run_loom_ok(root.path(), &["ops", "purge"]);

    let workers = 8usize;
    let barrier = Arc::new(Barrier::new(workers));
    let root_path = Arc::new(root.path().to_path_buf());

    let handles = (0..workers)
        .map(|_| {
            let barrier = Arc::clone(&barrier);
            let root_path = Arc::clone(&root_path);
            thread::spawn(move || {
                barrier.wait();
                let (output, env) =
                    run_loom_with_env(&root_path, &[], &["skill", "snapshot", "demo"]);
                (
                    output.status.code().unwrap_or(-1),
                    env["ok"].as_bool().unwrap_or(false),
                    env["error"]["code"].as_str().map(|s| s.to_string()),
                )
            })
        })
        .collect::<Vec<_>>();

    let mut success = 0usize;
    let mut lock_busy = 0usize;
    for handle in handles {
        let (code, ok, error_code) = handle.join().expect("join snapshot worker");
        if ok && code == 0 {
            success += 1;
        } else if error_code.as_deref() == Some("LOCK_BUSY") {
            lock_busy += 1;
        } else {
            panic!("unexpected concurrent snapshot result: code={code} error={error_code:?}");
        }
    }

    assert!(success >= 1);
    assert_eq!(success + lock_busy, workers);

    let pending = run_loom_ok(root.path(), &["ops", "list"]);
    assert_eq!(pending["data"]["count"].as_u64().unwrap(), success as u64);
    assert_eq!(pending["meta"]["warnings"].as_array().unwrap().len(), 0);
}

#[test]
fn ops_history_repair_compacts_excess_segments_into_archives() {
    let root = TestDir::new("history-retention-segments");
    let source = make_skill_source(root.path(), "source-history-retention-segments");

    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    let files = (0..9)
        .map(|index| {
            (
                format!("pending_ops_history/segment-{index:02}.jsonl"),
                history_event_line("segment", index),
            )
        })
        .collect::<Vec<_>>();
    replace_history_branch(
        root.path(),
        "loom-history",
        &files,
        "seed segment-heavy loom-history",
    );

    let diagnose_before = run_loom_ok(root.path(), &["ops", "history", "diagnose"]);
    assert_eq!(diagnose_before["data"]["local_segments"], 9);
    assert_eq!(diagnose_before["data"]["local_archives"], 0);

    let repair = run_loom_ok(
        root.path(),
        &["ops", "history", "repair", "--strategy", "local"],
    );
    assert_eq!(repair["data"]["result"], "compacted");
    assert_eq!(repair["data"]["compacted_segments"], 5);
    assert_eq!(repair["data"]["local_segments"], 4);
    assert_eq!(repair["data"]["local_archives"], 1);
    assert_eq!(repair["data"]["local_snapshot"], true);

    assert_eq!(
        git_branch_path_count(root.path(), "loom-history", "pending_ops_history/"),
        4
    );
    assert_eq!(
        git_branch_path_count(root.path(), "loom-history", "pending_ops_archive/"),
        1
    );
}

#[test]
fn ops_history_repair_rolls_excess_archives_forward() {
    let root = TestDir::new("history-retention-archives");
    let source = make_skill_source(root.path(), "source-history-retention-archives");

    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );

    let files = (0..5)
        .map(|index| {
            (
                format!("pending_ops_archive/archive-{index:02}.jsonl"),
                history_event_line("archive", index),
            )
        })
        .collect::<Vec<_>>();
    replace_history_branch(
        root.path(),
        "loom-history",
        &files,
        "seed archive-heavy loom-history",
    );

    let repair = run_loom_ok(
        root.path(),
        &["ops", "history", "repair", "--strategy", "local"],
    );
    assert_eq!(repair["data"]["result"], "compacted");
    assert_eq!(repair["data"]["compacted_segments"], 0);
    assert_eq!(repair["data"]["rolled_archives"], 2);
    assert_eq!(repair["data"]["local_segments"], 0);
    assert_eq!(repair["data"]["local_archives"], 4);
    assert_eq!(
        git_branch_path_count(root.path(), "loom-history", "pending_ops_archive/"),
        4
    );
}

#[test]
fn ops_history_diagnose_and_repair_resolve_path_conflicts() {
    let root = TestDir::new("history-diagnose-repair");
    let remote_root = TestDir::new("history-diagnose-repair-remote");
    let remote = remote_root.path().join("origin.git");
    let source = make_skill_source(root.path(), "source-history-diagnose-repair");

    git_ok(["init", "--bare", remote.to_str().unwrap()]);
    run_loom_ok(
        root.path(),
        &["workspace", "remote", "set", remote.to_str().unwrap()],
    );
    run_loom_ok(
        root.path(),
        &["skill", "add", source.to_str().unwrap(), "--name", "demo"],
    );
    run_loom_ok(root.path(), &["sync", "push"]);

    let conflict_path = "pending_ops_history/conflict.jsonl".to_string();
    replace_history_branch(
        remote.as_path(),
        "loom-history",
        &[(
            conflict_path.clone(),
            history_event_line("remote-conflict", 1),
        )],
        "seed remote conflict",
    );
    replace_history_branch(
        root.path(),
        "loom-history",
        &[(
            conflict_path.clone(),
            history_event_line("local-conflict", 2),
        )],
        "seed local conflict",
    );
    git_ok_in(root.path(), &["fetch", "origin", "loom-history"]);

    let diagnose = run_loom_ok(root.path(), &["ops", "history", "diagnose"]);
    assert_eq!(diagnose["data"]["local_branch"], true);
    assert_eq!(diagnose["data"]["remote_tracking"], true);
    assert_eq!(diagnose["data"]["conflicts"].as_array().unwrap().len(), 1);
    let conflict = &diagnose["data"]["conflicts"][0];
    assert_eq!(conflict["scope"], "segment");
    assert_eq!(conflict["path"], conflict_path);
    let local_rename_path = conflict["local_rename_path"]
        .as_str()
        .expect("local rename path")
        .to_string();

    let repair = run_loom_ok(
        root.path(),
        &["ops", "history", "repair", "--strategy", "remote"],
    );
    assert_eq!(repair["data"]["result"], "repaired");
    assert_eq!(repair["data"]["repaired_conflicts"], 1);
    assert_eq!(repair["data"]["local_snapshot"], true);
    assert!(git_branch_parent_count(root.path(), "loom-history") >= 2);

    let files = git_ok_in(
        root.path(),
        &["ls-tree", "-r", "--name-only", "loom-history"],
    );
    assert!(files.lines().any(|line| line == conflict_path));
    assert!(files.lines().any(|line| line == local_rename_path));

    let diagnose_after = run_loom_ok(root.path(), &["ops", "history", "diagnose"]);
    assert_eq!(
        diagnose_after["data"]["conflicts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
