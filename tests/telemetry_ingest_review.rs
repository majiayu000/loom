use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom_with_env, write_file, write_skill};

fn enable(root: &TestDir, home_arg: &str) {
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    assert!(output.status.success(), "enable failed: {envelope}");
}

fn claude_record(id: &str, call: &str, timestamp: &str, cwd: &str) -> String {
    claude_record_for(id, call, timestamp, cwd, "demo")
}

fn claude_record_for(id: &str, call: &str, timestamp: &str, cwd: &str, skill: &str) -> String {
    json!({
        "uuid": id,
        "sessionId": "review-session",
        "cwd": cwd,
        "timestamp": timestamp,
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use",
            "id": call,
            "name": "Skill",
            "input": {"skill": skill}
        }]}
    })
    .to_string()
        + "\n"
}

fn restore_modified(path: &std::path::Path, modified: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
}

fn codex_session(skill: &str, timestamp: &str) -> String {
    [
        json!({"timestamp":timestamp,"type":"session_meta","payload":{"id":"codex-review"}}),
        json!({"timestamp":timestamp,"type":"turn_context","payload":{"turn_id":"turn-1"}}),
        json!({
            "timestamp":timestamp,
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":format!("please use ${skill}")
            }]}
        }),
        json!({
            "timestamp":timestamp,
            "type":"response_item",
            "payload":{"type":"message","role":"user","content":[{
                "type":"input_text","text":format!("<skill><name>{skill}</name>body</skill>")
            }]}
        }),
    ]
    .into_iter()
    .map(|value| value.to_string() + "\n")
    .collect()
}

#[test]
fn imported_workspace_is_hashed_and_report_filterable() {
    let root = TestDir::new("telemetry-ingest-review-workspace");
    let home = TestDir::new("telemetry-ingest-review-workspace-home");
    let workspace = home.path().join("private-workspace");
    let other = home.path().join("other-workspace");
    let source = home.path().join("projects/demo/session.jsonl");
    write_file(
        &source,
        &claude_record(
            "workspace-record",
            "workspace-call",
            "2026-07-01T00:00:00Z",
            &workspace.to_string_lossy(),
        ),
    );
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);
    let (ingest_output, ingest) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(ingest_output.status.success(), "ingest failed: {ingest}");

    let workspace_arg = workspace.to_string_lossy().into_owned();
    let (matching_output, matching) = run_loom_with_env(
        root.path(),
        &[],
        &["telemetry", "report", "--workspace", &workspace_arg],
    );
    assert!(
        matching_output.status.success(),
        "report failed: {matching}"
    );
    assert_eq!(matching["data"]["matched_events"], json!(1));
    let other_arg = other.to_string_lossy().into_owned();
    let (_, non_matching) = run_loom_with_env(
        root.path(),
        &[],
        &["telemetry", "report", "--workspace", &other_arg],
    );
    assert_eq!(non_matching["data"]["matched_events"], json!(0));

    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert!(!events.contains(&workspace_arg));
    let event: Value = serde_json::from_str(events.trim()).unwrap();
    assert!(event["workspace_hash"].is_string());
}

#[test]
fn simultaneous_logical_source_copies_keep_the_active_checkpoint() {
    let root = TestDir::new("telemetry-ingest-review-copies");
    let home = TestDir::new("telemetry-ingest-review-copies-home");
    let current = home.path().join("projects/demo/session.jsonl");
    let rotated = home.path().join("projects/demo/z-rotated.jsonl");
    let workspace = home.path().join("workspace").to_string_lossy().into_owned();
    let first = claude_record(
        "copy-one",
        "copy-call-one",
        "2026-07-01T00:00:00Z",
        &workspace,
    );
    let unmatched = claude_record_for(
        "copy-unmatched",
        "copy-call-unmatched",
        "2026-07-01T00:00:30Z",
        &workspace,
        "retired-skill",
    );
    let first_copy = format!("{first}{unmatched}");
    write_file(&rotated, &first_copy);
    let second = claude_record(
        "copy-two",
        "copy-call-two",
        "2026-07-01T00:01:00Z",
        &workspace,
    );
    write_file(&current, &format!("{first_copy}{second}"));
    write_file(&rotated, &first_copy);
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);

    let (first_output, first_envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        first_output.status.success(),
        "first ingest: {first_envelope}"
    );
    assert_eq!(first_envelope["data"]["ingested"], json!(3));
    assert_eq!(first_envelope["data"]["unmatched"][0]["count"], json!(1));
    let cursor: Value = serde_json::from_str(
        &fs::read_to_string(root.path().join("state/telemetry/ingest_cursor.json")).unwrap(),
    )
    .unwrap();
    let checkpoint = cursor["sources"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(
        checkpoint["committed_offset"],
        json!(u64::try_from(first_copy.len() + second.len()).unwrap())
    );
    write_file(&rotated, &first_copy);
    let (_, second_envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert_eq!(second_envelope["data"]["ingested"], json!(0));
    assert_eq!(second_envelope["data"]["sources_reset"]["count"], json!(0));

    let third = claude_record(
        "copy-three",
        "copy-call-three",
        "2026-07-01T00:02:00Z",
        &workspace,
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&current)
        .unwrap()
        .write_all(third.as_bytes())
        .unwrap();
    let (_, appended) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert_eq!(appended["data"]["ingested"], json!(1));
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(events.lines().count(), 4);
}

#[test]
fn scanner_is_streamed_and_oversized_records_are_counted() {
    assert!(!include_str!("../src/commands/telemetry/ingest/mod.rs").contains("read_to_end"));
    let root = TestDir::new("telemetry-ingest-review-stream");
    let home = TestDir::new("telemetry-ingest-review-stream-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let workspace = home.path().join("workspace").to_string_lossy().into_owned();
    let valid = claude_record(
        "after-oversized",
        "after-oversized-call",
        "2026-07-01T00:00:00Z",
        &workspace,
    );
    write_file(&source, &("x".repeat(2 * 1024 * 1024) + "\n" + &valid));
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(output.status.success(), "oversized scan failed: {envelope}");
    assert_eq!(envelope["data"]["malformed"], json!(1));
    assert_eq!(envelope["data"]["ingested"], json!(1));
}

#[test]
fn codex_since_uses_record_time_and_accepts_dotted_skills() {
    let root = TestDir::new("telemetry-ingest-review-codex-mtime");
    let home = TestDir::new("telemetry-ingest-review-codex-mtime-home");
    let source = home.path().join("sessions/2026/07/session.jsonl");
    write_file(
        &source,
        &codex_session("team.skill", "2026-07-02T00:00:00Z"),
    );
    restore_modified(&source, SystemTime::UNIX_EPOCH);
    write_skill(
        root.path(),
        "team.skill",
        "---\nname: team.skill\ndescription: Dotted fixture skill.\n---\n# Dotted\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CODEX_HOME", &home_arg)],
        &[
            "telemetry",
            "ingest",
            "--agent",
            "codex",
            "--since",
            "2026-07-01T00:00:00Z",
        ],
    );
    assert!(output.status.success(), "Codex ingest failed: {envelope}");
    assert_eq!(envelope["data"]["ingested"], json!(1));
}

#[test]
fn source_rotated_after_discovery_is_rediscovered() {
    let root = TestDir::new("telemetry-ingest-review-rotation");
    let home = TestDir::new("telemetry-ingest-review-rotation-home");
    let source = home.path().join("projects/demo/session.jsonl");
    write_file(
        &source,
        &claude_record(
            "rotation-record",
            "rotation-call",
            "2026-07-01T00:00:00Z",
            "/workspace",
        ),
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);
    let child = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["telemetry", "ingest", "--agent", "claude"])
        .env("LOOM_CLAUDE_HOME", &home_arg)
        .env("LOOM_TEST_INGEST_OPEN_PAUSE_MS", "1000")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn discovery-paused ingest");
    thread::sleep(Duration::from_millis(300));
    let rotated = home.path().join("projects/demo/rotated.jsonl");
    fs::rename(&source, &rotated).expect("rotate source after discovery");
    let output = child.wait_with_output().expect("wait for ingest");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("parse ingest envelope");
    assert!(
        output.status.success(),
        "rotation ingest failed: {envelope}"
    );
    assert_eq!(envelope["data"]["ingested"], json!(1));
}

#[test]
fn source_rewrite_during_scan_retries_before_commit() {
    let root = TestDir::new("telemetry-ingest-review-snapshot-race");
    let home = TestDir::new("telemetry-ingest-review-snapshot-race-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let workspace = home.path().join("workspace").to_string_lossy().into_owned();
    let mut body_a = String::new();
    for index in 0..120 {
        body_a.push_str(&claude_record(
            &format!("snapshot-record-{index:03}"),
            &format!("snapshot-call-{index:03}"),
            "2026-07-01T00:00:00Z",
            &workspace,
        ));
    }
    let body_b = body_a
        .replacen("snapshot-record-060", "snapshot-record-alt", 1)
        .replacen("snapshot-call-060", "snapshot-call-alt", 1);
    assert_eq!(body_a.len(), body_b.len());
    assert!(body_a.len() > 16 * 1024);
    write_file(&source, &body_a);
    let original_modified = fs::metadata(&source).unwrap().modified().unwrap();
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);

    let child = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["telemetry", "ingest", "--agent", "claude"])
        .env("LOOM_CLAUDE_HOME", &home_arg)
        .env("LOOM_TEST_INGEST_SCAN_PAUSE_MS", "1000")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn paused ingest");
    thread::sleep(Duration::from_millis(300));
    write_file(&source, &body_b);
    restore_modified(&source, original_modified);
    let output = child.wait_with_output().expect("wait for paused ingest");
    let first: Value = serde_json::from_slice(&output.stdout).expect("parse first ingest");
    assert!(output.status.success(), "first ingest failed: {first}");
    assert_eq!(first["data"]["ingested"], json!(120));

    write_file(&source, &body_a);
    let (second_output, second) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        second_output.status.success(),
        "second ingest failed: {second}"
    );
    assert_eq!(second["data"]["sources_reset"]["count"], json!(1));
    assert_eq!(second["data"]["ingested"], json!(1));
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(events.lines().count(), 121);
}

#[test]
fn coalesced_source_drafts_keep_every_physical_snapshot_guard() {
    let root = TestDir::new("telemetry-ingest-review-coalesced-race");
    let home = TestDir::new("telemetry-ingest-review-coalesced-race-home");
    let rotated = home.path().join("projects/demo/a-rotated.jsonl");
    let current = home.path().join("projects/demo/z-current.jsonl");
    let workspace = home.path().join("workspace").to_string_lossy().into_owned();
    let common = claude_record(
        "coalesced-common",
        "coalesced-common-call",
        "2026-07-01T00:00:00Z",
        &workspace,
    );
    let rotated_a = format!(
        "{common}{}",
        claude_record(
            "coalesced-rotated-a",
            "coalesced-rotated-call-a",
            "2026-07-01T00:01:00Z",
            &workspace,
        )
    );
    let rotated_b = rotated_a
        .replacen("coalesced-rotated-a", "coalesced-rotated-b", 1)
        .replacen("coalesced-rotated-call-a", "coalesced-rotated-call-b", 1);
    let current_body = format!(
        "{common}{}{}",
        claude_record(
            "coalesced-current-1",
            "coalesced-current-call-1",
            "2026-07-01T00:02:00Z",
            &workspace,
        ),
        claude_record(
            "coalesced-current-2",
            "coalesced-current-call-2",
            "2026-07-01T00:03:00Z",
            &workspace,
        )
    );
    assert_eq!(rotated_a.len(), rotated_b.len());
    assert!(current_body.len() > rotated_a.len());
    write_file(&rotated, &rotated_a);
    write_file(&current, &current_body);
    let original_modified = fs::metadata(&rotated).unwrap().modified().unwrap();
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    enable(&root, &home_arg);

    let child = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["telemetry", "ingest", "--agent", "claude"])
        .env("LOOM_CLAUDE_HOME", &home_arg)
        .env("LOOM_TEST_INGEST_COMMIT_PAUSE_MS", "1000")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn commit-paused ingest");
    thread::sleep(Duration::from_millis(300));
    write_file(&rotated, &rotated_b);
    restore_modified(&rotated, original_modified);
    let output = child
        .wait_with_output()
        .expect("wait for commit-paused ingest");
    let first: Value = serde_json::from_slice(&output.stdout).expect("parse first ingest");
    assert!(output.status.success(), "first ingest failed: {first}");
    assert_eq!(first["data"]["ingested"], json!(4));

    write_file(&rotated, &rotated_a);
    let (second_output, second) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        second_output.status.success(),
        "second ingest failed: {second}"
    );
    assert_eq!(second["data"]["ingested"], json!(1));
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(events.lines().count(), 5);
}
