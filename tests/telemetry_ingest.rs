use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom_with_env, write_file, write_skill};

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

fn fixture_homes(temp: &TestDir) -> (String, String) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry_ingest");
    let claude = temp.path().join("agent-homes/claude");
    let codex = temp.path().join("agent-homes/codex");
    copy_tree(&fixture.join("claude"), &claude);
    copy_tree(&fixture.join("codex"), &codex);
    (
        claude.to_string_lossy().into_owned(),
        codex.to_string_lossy().into_owned(),
    )
}

#[test]
fn parser_fixtures_and_repeated_ingest_are_deterministic() {
    let root = TestDir::new("telemetry-ingest-fixtures");
    let homes = TestDir::new("telemetry-ingest-homes");
    let (claude, codex) = fixture_homes(&homes);
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let (enable_output, enable) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &claude), ("LOOM_CODEX_HOME", &codex)],
        &["telemetry", "enable", "--local-only"],
    );
    assert!(enable_output.status.success(), "enable failed: {enable}");

    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &claude), ("LOOM_CODEX_HOME", &codex)],
        &["telemetry", "ingest", "--agent", "all"],
    );
    assert!(output.status.success(), "ingest failed: {envelope}");
    assert_eq!(envelope["cmd"], json!("telemetry.ingest"));
    assert_eq!(envelope["data"]["ingested"], json!(6), "{envelope}");
    assert_eq!(envelope["data"]["malformed"], json!(1));
    assert_eq!(envelope["data"]["rejected"]["count"], json!(3));
    assert_eq!(envelope["data"]["unmatched"].as_array().unwrap().len(), 2);
    assert!(!envelope.to_string().contains("bad/name"));

    let events_path = root.path().join("state/telemetry/events.jsonl");
    let first = fs::read_to_string(&events_path).expect("read imported events");
    let events = first.lines().collect::<Vec<_>>();
    assert_eq!(events.len(), 6);
    let ids = events
        .iter()
        .map(|line| {
            serde_json::from_str::<Value>(line).unwrap()["event_id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 6, "same-time invocations need distinct ids");
    assert!(!first.contains(&claude));
    assert!(!first.contains(&codex));
    assert!(!first.contains("/workspace/"));
    assert!(!first.contains("bad/name"));
    assert!(events.iter().all(|line| {
        serde_json::from_str::<Value>(line).unwrap()["workspace_hash"].is_string()
    }));
    let cursor = fs::read_to_string(root.path().join("state/telemetry/ingest_cursor.json"))
        .expect("read ingest cursor");
    assert!(!cursor.contains(&claude));
    assert!(!cursor.contains(&codex));

    let (report_output, report) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &claude), ("LOOM_CODEX_HOME", &codex)],
        &["telemetry", "report", "--skill", "retired-skill"],
    );
    assert!(report_output.status.success(), "report failed: {report}");
    assert_eq!(report["data"]["matched_events"], json!(2));
    assert_eq!(
        report["data"]["skills"]["retired-skill"]["usage"]["invocations"],
        json!(2)
    );

    let (second_output, second) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &claude), ("LOOM_CODEX_HOME", &codex)],
        &["telemetry", "ingest", "--agent", "all"],
    );
    assert!(second_output.status.success(), "repeat failed: {second}");
    assert_eq!(second["data"]["ingested"], json!(0));
    assert_eq!(second["data"]["sources_reset"]["count"], json!(0));
    assert_eq!(fs::read_to_string(events_path).unwrap(), first);
}

#[test]
fn disabled_fails_closed_and_dry_run_is_read_only() {
    let root = TestDir::new("telemetry-ingest-dry-run");
    let homes = TestDir::new("telemetry-ingest-dry-run-homes");
    let (claude, _) = fixture_homes(&homes);
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let before = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    let (dry_output, dry) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &claude)],
        &["telemetry", "ingest", "--agent", "claude", "--dry-run"],
    );
    assert!(dry_output.status.success(), "dry-run failed: {dry}");
    assert_eq!(dry["data"]["dry_run"], Value::Bool(true));
    assert_eq!(dry["data"]["ingested"], json!(3));
    assert!(!root.path().join("state/telemetry").exists());
    let after = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(before, after);

    let (write_output, failure) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &claude)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(!write_output.status.success());
    assert_eq!(failure["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        failure["error"]["next_actions"][0]["cmd"],
        json!("loom telemetry enable --local-only --json")
    );
}

#[test]
fn trailing_partial_record_is_committed_only_after_newline() {
    let root = TestDir::new("telemetry-ingest-partial");
    let home = TestDir::new("telemetry-ingest-partial-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let record = json!({
        "uuid": "partial-record",
        "sessionId": "partial-session",
        "timestamp": "2026-07-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "partial-call", "name": "Skill",
            "input": {"skill": "demo"}
        }]}
    })
    .to_string();
    write_file(&source, &record);
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let (partial_output, partial) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        partial_output.status.success(),
        "partial scan failed: {partial}"
    );
    assert_eq!(partial["data"]["pending_partial"], json!(1));
    assert_eq!(partial["data"]["ingested"], json!(0));
    let cursor = fs::read_to_string(root.path().join("state/telemetry/ingest_cursor.json"))
        .expect("read cursor");
    assert!(cursor.contains(r#""committed_offset": 0"#));
    assert!(!cursor.contains(&home_arg));

    write_file(&source, &(record + "\n"));
    let (complete_output, complete) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        complete_output.status.success(),
        "completed scan failed: {complete}"
    );
    assert_eq!(complete["data"]["ingested"], json!(1));
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(events.lines().count(), 1);
}

#[test]
fn older_since_backfills_and_same_size_rewrite_resets() {
    let root = TestDir::new("telemetry-ingest-backfill");
    let home = TestDir::new("telemetry-ingest-backfill-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let old = json!({
        "uuid": "old-record",
        "sessionId": "backfill-session",
        "timestamp": "2026-06-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "old-call", "name": "Skill",
            "input": {"skill": "demo"}
        }]}
    })
    .to_string();
    let recent = json!({
        "uuid": "new-record",
        "sessionId": "backfill-session",
        "timestamp": "2026-07-02T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "new-call", "name": "Skill",
            "input": {"skill": "demo"}
        }]}
    })
    .to_string();
    let body = format!("{old}\n{recent}\n");
    write_file(&source, &body);
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let (recent_output, recent_envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &[
            "telemetry",
            "ingest",
            "--agent",
            "claude",
            "--since",
            "2026-07-01",
        ],
    );
    assert!(
        recent_output.status.success(),
        "recent ingest: {recent_envelope}"
    );
    assert_eq!(recent_envelope["data"]["ingested"], json!(1));
    assert_eq!(recent_envelope["data"]["window_skipped"], json!(1));

    let (older_output, older) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(older_output.status.success(), "older ingest: {older}");
    assert_eq!(older["data"]["ingested"], json!(1));
    assert_eq!(older["data"]["duplicates_skipped"], json!(1));

    let rewritten = body.replacen("old-record", "alt-record", 1);
    assert_eq!(rewritten.len(), body.len());
    write_file(&source, &rewritten);
    let (rewrite_output, rewrite) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(rewrite_output.status.success(), "rewrite ingest: {rewrite}");
    assert_eq!(rewrite["data"]["sources_reset"]["count"], json!(1));
}

#[test]
fn concurrent_ingest_retries_cursor_compare_and_commit() {
    let root = TestDir::new("telemetry-ingest-concurrent");
    let home = TestDir::new("telemetry-ingest-concurrent-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let mut body = String::new();
    for index in 0..200 {
        body.push_str(
            &json!({
                "uuid": format!("concurrent-record-{index}"),
                "sessionId": "concurrent-session",
                "timestamp": "2026-07-01T00:00:00Z",
                "type": "assistant",
                "message": {"content": [{
                    "type": "tool_use",
                    "id": format!("concurrent-call-{index}"),
                    "name": "Skill",
                    "input": {"skill": "demo"}
                }]}
            })
            .to_string(),
        );
        body.push('\n');
    }
    write_file(&source, &body);
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let spawn = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
        command
            .arg("--json")
            .arg("--root")
            .arg(root.path())
            .args(["telemetry", "ingest", "--agent", "claude"])
            .env("LOOM_CLAUDE_HOME", &home_arg)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn concurrent ingest")
    };
    let first = spawn();
    let second = spawn();
    let first = first.wait_with_output().expect("wait first ingest");
    let second = second.wait_with_output().expect("wait second ingest");
    assert!(
        first.status.success(),
        "first ingest failed: {}",
        String::from_utf8_lossy(&first.stdout)
    );
    assert!(
        second.status.success(),
        "second ingest failed: {}",
        String::from_utf8_lossy(&second.stdout)
    );
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(events.lines().count(), 200);
}

#[test]
fn interrupted_cursor_write_retries_without_duplicate_events() {
    let root = TestDir::new("telemetry-ingest-interrupted");
    let home = TestDir::new("telemetry-ingest-interrupted-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let record = json!({
        "uuid": "interrupted-record",
        "sessionId": "interrupted-session",
        "timestamp": "2026-07-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "interrupted-call", "name": "Skill",
            "input": {"skill": "demo"}
        }]}
    });
    write_file(&source, &format!("{record}\n"));
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let (failed_output, _) = run_loom_with_env(
        root.path(),
        &[
            ("LOOM_CLAUDE_HOME", &home_arg),
            ("LOOM_FAULT_INJECT", "write_atomic"),
        ],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(!failed_output.status.success());
    assert!(
        !root
            .path()
            .join("state/telemetry/ingest_cursor.json")
            .exists()
    );
    let persisted = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(persisted.lines().count(), 1);

    let (retry_output, retry) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(retry_output.status.success(), "retry failed: {retry}");
    assert_eq!(retry["data"]["ingested"], json!(0));
    assert_eq!(retry["data"]["duplicates_skipped"], json!(1));
    let persisted = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(persisted.lines().count(), 1);
}

#[test]
fn loom_home_override_precedes_native_home_and_missing_root_is_empty() {
    let root = TestDir::new("telemetry-ingest-home-precedence");
    let override_home = TestDir::new("telemetry-ingest-override-home");
    let native_home = TestDir::new("telemetry-ingest-native-home");
    let record = |id: &str, skill: &str| {
        json!({
            "uuid": id,
            "sessionId": "home-session",
            "timestamp": "2026-07-01T00:00:00Z",
            "type": "assistant",
            "message": {"content": [{
                "type": "tool_use", "id": format!("{id}-call"), "name": "Skill",
                "input": {"skill": skill}
            }]}
        })
        .to_string()
            + "\n"
    };
    write_file(
        &override_home.path().join("projects/demo/session.jsonl"),
        &record("override-record", "override-skill"),
    );
    write_file(
        &native_home.path().join("projects/demo/session.jsonl"),
        &record("native-record", "native-skill"),
    );
    for skill in ["override-skill", "native-skill"] {
        write_skill(
            root.path(),
            skill,
            &format!("---\nname: {skill}\ndescription: Home fixture.\n---\n# Home\n"),
        );
    }
    let override_arg = override_home.path().to_string_lossy().into_owned();
    let native_arg = native_home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[
            ("LOOM_CLAUDE_HOME", &override_arg),
            ("CLAUDE_HOME", &native_arg),
        ],
        &["telemetry", "enable", "--local-only"],
    );
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[
            ("LOOM_CLAUDE_HOME", &override_arg),
            ("CLAUDE_HOME", &native_arg),
        ],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        output.status.success(),
        "override ingest failed: {envelope}"
    );
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert!(events.contains(r#""skill_id":"override-skill""#));
    assert!(!events.contains("native-skill"));

    let missing = root.path().join("missing-claude-home");
    let missing_arg = missing.to_string_lossy().into_owned();
    let (missing_output, missing_envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &missing_arg)],
        &["telemetry", "ingest", "--agent", "claude", "--dry-run"],
    );
    assert!(
        missing_output.status.success(),
        "missing root failed: {missing_envelope}"
    );
    assert_eq!(missing_envelope["data"]["scanned_files"], json!(0));
}

#[test]
fn trust_only_skill_is_unmatched_and_dot_name_is_rejected() {
    let root = TestDir::new("telemetry-ingest-trust-only");
    let home = TestDir::new("telemetry-ingest-trust-only-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let trusted_only = json!({
        "uuid": "trust-only-record",
        "sessionId": "trust-only-session",
        "timestamp": "2026-07-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "trust-only-call", "name": "Skill",
            "input": {"skill": "ghost"}
        }]}
    });
    let dot = json!({
        "uuid": "dot-record",
        "sessionId": "trust-only-session",
        "timestamp": "2026-07-01T00:01:00Z",
        "type": "user",
        "message": {"content": "<command-name>/.</command-name>"}
    });
    write_file(&source, &format!("{trusted_only}\n{dot}\n"));
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"ghost","trust":"local-draft","quarantined":false,"updated_at":"2026-07-01T00:00:00Z","updated_by":"test"}]}
"#,
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(output.status.success(), "ingest failed: {envelope}");
    assert_eq!(envelope["data"]["ingested"], json!(1));
    assert_eq!(
        envelope["data"]["rejected"]["reasons"]["invalid_observed_skill_name"],
        json!(1)
    );
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert!(events.contains(r#""observed_skill_name":"ghost""#));
    assert!(!events.contains(r#""skill_id":"ghost""#));
    assert!(!events.contains(r#""observed_skill_name":".""#));
    let (report_output, report) = run_loom_with_env(
        root.path(),
        &[],
        &["telemetry", "report", "--skill", "ghost"],
    );
    assert!(report_output.status.success(), "report failed: {report}");
    assert_eq!(report["data"]["matched_events"], json!(1));
}

#[test]
fn unterminated_event_log_tail_fails_without_cursor_advance() {
    let root = TestDir::new("telemetry-ingest-event-tail");
    let home = TestDir::new("telemetry-ingest-event-tail-home");
    let source = home.path().join("projects/demo/session.jsonl");
    let record = json!({
        "uuid": "tail-record",
        "sessionId": "tail-session",
        "timestamp": "2026-07-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "tail-call", "name": "Skill",
            "input": {"skill": "demo"}
        }]}
    });
    write_file(&source, &format!("{record}\n"));
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let home_arg = home.path().to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let event_tail = root.path().join("state/telemetry/events.jsonl");
    write_file(&event_tail, r#"{"schema_version":3"#);
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(!output.status.success());
    assert_eq!(envelope["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        fs::read_to_string(event_tail).unwrap(),
        r#"{"schema_version":3"#
    );
    assert!(
        !root
            .path()
            .join("state/telemetry/ingest_cursor.json")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn source_alias_and_rotated_copy_share_cursor_and_event_identity() {
    use std::os::unix::fs::symlink;

    let root = TestDir::new("telemetry-ingest-source-alias");
    let homes = TestDir::new("telemetry-ingest-source-alias-homes");
    let real = homes.path().join("real");
    let alias = homes.path().join("alias");
    let source = real.join("projects/demo/session.jsonl");
    let record = json!({
        "uuid": "alias-record",
        "sessionId": "alias-session",
        "timestamp": "2026-07-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use", "id": "alias-call", "name": "Skill",
            "input": {"skill": "demo"}
        }]}
    })
    .to_string()
        + "\n";
    write_file(&source, &record);
    symlink(&real, &alias).expect("create home alias");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let alias_arg = alias.to_string_lossy().into_owned();
    run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &alias_arg)],
        &["telemetry", "enable", "--local-only"],
    );
    let (first_output, first) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &alias_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        first_output.status.success(),
        "alias ingest failed: {first}"
    );
    let rotated = real.join("projects/demo/rotated.jsonl");
    fs::copy(&source, &rotated).expect("copy rotated log");
    fs::remove_file(&source).expect("remove original log");
    let real_arg = real.to_string_lossy().into_owned();
    let (second_output, second) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &real_arg)],
        &["telemetry", "ingest", "--agent", "claude"],
    );
    assert!(
        second_output.status.success(),
        "rotated ingest failed: {second}"
    );
    assert_eq!(second["data"]["ingested"], json!(0));
    assert_eq!(second["data"]["sources_reset"]["count"], json!(1));
    assert_eq!(
        second["data"]["sources_reset"]["reasons"]["generation_changed"],
        json!(1)
    );
    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert_eq!(events.lines().count(), 1);
    let cursor: Value = serde_json::from_str(
        &fs::read_to_string(root.path().join("state/telemetry/ingest_cursor.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cursor["sources"].as_object().unwrap().len(), 1);
    assert_eq!(fs::read_to_string(rotated).unwrap(), record);
}

#[cfg(unix)]
#[test]
fn unreadable_existing_agent_root_fails_instead_of_reporting_empty() {
    use std::os::unix::fs::PermissionsExt;

    let root = TestDir::new("telemetry-ingest-unreadable-root");
    let home = TestDir::new("telemetry-ingest-unreadable-home");
    let projects = home.path().join("projects");
    fs::create_dir_all(&projects).expect("create projects root");
    let mut permissions = fs::metadata(&projects).unwrap().permissions();
    permissions.set_mode(0o000);
    fs::set_permissions(&projects, permissions).expect("make projects unreadable");
    let home_arg = home.path().to_string_lossy().into_owned();
    let (output, envelope) = run_loom_with_env(
        root.path(),
        &[("LOOM_CLAUDE_HOME", &home_arg)],
        &["telemetry", "ingest", "--agent", "claude", "--dry-run"],
    );
    let mut restore = fs::metadata(&projects).unwrap().permissions();
    restore.set_mode(0o700);
    fs::set_permissions(&projects, restore).expect("restore projects permissions");
    assert!(
        !output.status.success(),
        "unreadable root passed: {envelope}"
    );
    assert_eq!(envelope["error"]["code"], json!("IO_ERROR"));
}
