use std::fs;

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom_with_env, write_file, write_skill};

fn claude_skill_call(session: &str, entrypoint: &str, id: &str) -> String {
    json!({
        "uuid": format!("record-{id}"),
        "sessionId": session,
        "entrypoint": entrypoint,
        "cwd": "/workspace/demo",
        "timestamp": "2026-07-01T00:00:00Z",
        "type": "assistant",
        "message": {"content": [{
            "type": "tool_use",
            "name": "Skill",
            "id": id,
            "input": {"skill": "demo"}
        }]}
    })
    .to_string()
}

fn codex_read(call_id: &str, skill: &str, minute: u32) -> String {
    json!({
        "type": "response_item",
        "timestamp": format!("2026-07-01T00:{minute:02}:00Z"),
        "payload": {
            "type": "function_call",
            "name": "exec_command",
            "call_id": call_id,
            "arguments": json!({"cmd": format!("cat ~/.agents/skills/{skill}/SKILL.md")}).to_string()
        }
    })
    .to_string()
}

/// One interactive and one SDK Claude call, plus a `codex exec` session whose first turn reads
/// eight different skills (an inventory sweep) and whose second turn reads `demo` once.
fn write_homes(homes: &TestDir) -> (String, String) {
    let claude = homes.path().join("claude");
    let codex = homes.path().join("codex");
    write_file(
        &claude.join("projects/demo/interactive.jsonl"),
        &format!("{}\n", claude_skill_call("s-interactive", "cli", "call-1")),
    );
    write_file(
        &claude.join("projects/demo/scheduled.jsonl"),
        &format!(
            "{}\n",
            claude_skill_call("s-scheduled", "sdk-cli", "call-2")
        ),
    );
    let mut lines = vec![
        json!({"type": "session_meta", "payload": {"id": "codex-session", "originator": "codex_exec", "cwd": "/workspace/demo"}}).to_string(),
        json!({"type": "turn_context", "payload": {"turn_id": "turn-sweep", "cwd": "/workspace/demo"}}).to_string(),
    ];
    for index in 0..8 {
        lines.push(codex_read(
            &format!("sweep-{index}"),
            &format!("inventory-{index}"),
            1,
        ));
    }
    lines.push(
        json!({"type": "turn_context", "payload": {"turn_id": "turn-use", "cwd": "/workspace/demo"}})
            .to_string(),
    );
    lines.push(codex_read("use-1", "demo", 2));
    write_file(
        &codex.join("sessions/2026/07/01/rollout-2026-07-01T00-00-00-codex-session.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    (
        claude.to_string_lossy().into_owned(),
        codex.to_string_lossy().into_owned(),
    )
}

fn usage(report: &Value, skill: &str) -> Value {
    report["data"]["skills"][skill]["usage"].clone()
}

#[test]
fn origin_is_recorded_and_inventory_sweeps_are_not_usage() {
    let root = TestDir::new("telemetry-origin");
    let homes = TestDir::new("telemetry-origin-homes");
    let (claude, codex) = write_homes(&homes);
    let env = [
        ("LOOM_CLAUDE_HOME", claude.as_str()),
        ("LOOM_CODEX_HOME", codex.as_str()),
    ];
    let (output, init) = run_loom_with_env(root.path(), &env, &["workspace", "init"]);
    assert!(output.status.success(), "init failed: {init}");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Fixture skill.\n---\n# Demo\n",
    );
    let (output, enable) =
        run_loom_with_env(root.path(), &env, &["telemetry", "enable", "--local-only"]);
    assert!(output.status.success(), "enable failed: {enable}");
    let (output, ingest) = run_loom_with_env(
        root.path(),
        &env,
        &["telemetry", "ingest", "--agent", "all"],
    );
    assert!(output.status.success(), "ingest failed: {ingest}");
    assert_eq!(ingest["data"]["ingested"], json!(11), "{ingest}");

    let events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl")).unwrap();
    assert!(
        events
            .lines()
            .all(|line| line.contains(r#""schema_version":4"#))
    );
    assert!(!events.contains("turn-sweep"), "turn ids must be hashed");
    assert!(
        !events.contains("codex_exec"),
        "only the classified origin is stored"
    );

    let (output, report) = run_loom_with_env(root.path(), &env, &["telemetry", "report"]);
    assert!(output.status.success(), "report failed: {report}");
    let demo = usage(&report, "demo");
    assert_eq!(demo["invocations"], json!(3), "{demo}");
    assert_eq!(
        demo["invocations_by_origin"],
        json!({"interactive": 1, "automated": 2, "unknown": 0})
    );
    let inventory = usage(&report, "inventory-0");
    assert_eq!(inventory["invocations"], json!(0), "{inventory}");
    assert_eq!(inventory["audit_sweep_reads_excluded"], json!(1));
    assert_eq!(
        report["data"]["summary"]["usage"]["audit_sweep_reads_excluded"],
        json!(8)
    );

    let (output, interactive) = run_loom_with_env(
        root.path(),
        &env,
        &["telemetry", "report", "--origin", "interactive"],
    );
    assert!(
        output.status.success(),
        "filtered report failed: {interactive}"
    );
    assert_eq!(
        interactive["data"]["filters"]["origin"],
        json!("interactive")
    );
    assert_eq!(usage(&interactive, "demo")["invocations"], json!(1));

    let (output, stats) = run_loom_with_env(
        root.path(),
        &env,
        &["skill", "stats", "--origin", "automated"],
    );
    assert!(output.status.success(), "stats failed: {stats}");
    assert_eq!(stats["data"]["origin"], json!("automated"));
    let stats_text = stats.to_string();
    assert!(
        !stats_text.contains("inventory-0"),
        "sweep reads must not surface as orphan usage: {stats_text}"
    );
}
