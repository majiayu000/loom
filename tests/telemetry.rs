use std::fs;

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom, write_file, write_skill};

fn event_line(id: &str, event_type: &str, skill: &str, timestamp: &str, success: bool) -> String {
    json!({
        "schema_version": 1,
        "event_id": id,
        "event_type": event_type,
        "skill_id": skill,
        "agent": "codex",
        "workspace_hash": "sha256:test-workspace",
        "session_id_hash": "sha256:test-session",
        "timestamp": timestamp,
        "metrics": {
            "tokens_in": 10,
            "tokens_out": 2,
            "commands": 1,
            "duration_ms": 50,
            "success": success
        },
        "privacy": {
            "raw_prompt_stored": false,
            "raw_code_stored": false,
            "redacted": true
        }
    })
    .to_string()
}

#[test]
fn telemetry_status_absent_is_disabled_and_read_only() {
    let root = TestDir::new("telemetry-status");
    let (output, env) = run_loom(root.path(), &["telemetry", "status"]);

    assert!(output.status.success(), "status should pass: {env}");
    assert_eq!(env["cmd"], Value::String("telemetry.status".to_string()));
    assert_eq!(env["data"]["configured"], Value::Bool(false));
    assert_eq!(env["data"]["enabled"], Value::Bool(false));
    assert_eq!(env["data"]["retention_days"], json!(90));
    assert!(!root.path().join("state/telemetry").exists());
}

#[test]
fn telemetry_enable_disable_and_disabled_mode_prevents_event_appends() {
    let root = TestDir::new("telemetry-enable-disable");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing telemetry.\n---\n# Demo\n",
    );

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );
    assert_eq!(enable["data"]["enabled"], Value::Bool(true));

    let (disable_output, disable) = run_loom(root.path(), &["telemetry", "disable"]);
    assert!(
        disable_output.status.success(),
        "disable should pass: {disable}"
    );
    assert_eq!(disable["data"]["enabled"], Value::Bool(false));

    let (scan_output, scan) = run_loom(root.path(), &["skill", "scan", "demo"]);
    assert!(scan_output.status.success(), "scan should pass: {scan}");
    assert!(
        !root.path().join("state/telemetry/events.jsonl").exists(),
        "disabled telemetry must not append events"
    );
}

#[test]
fn telemetry_writes_redacted_eval_and_safety_events_and_reports_them() {
    let root = TestDir::new("telemetry-report");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing telemetry redaction.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"case-1","task":"do not store raw prompt sk_test_secret","output":"ok","metrics":{"tokens":123,"commands":2}}"#,
    );

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );

    let (eval_output, eval) = run_loom(root.path(), &["skill", "eval", "demo"]);
    assert!(eval_output.status.success(), "eval should pass: {eval}");
    let (scan_output, scan) = run_loom(root.path(), &["skill", "scan", "demo"]);
    assert!(scan_output.status.success(), "scan should pass: {scan}");

    let raw_events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl"))
        .expect("read telemetry events");
    assert!(raw_events.contains(r#""event_type":"skill.eval""#));
    assert!(raw_events.contains(r#""event_type":"skill.safety""#));
    assert!(raw_events.contains("sha256:"));
    assert!(!raw_events.contains("sk_test_secret"));
    assert!(!raw_events.contains("raw prompt"));

    let (report_output, report) =
        run_loom(root.path(), &["telemetry", "report", "--skill", "demo"]);
    assert!(
        report_output.status.success(),
        "report should pass: {report}"
    );
    assert_eq!(report["data"]["matched_events"], json!(2));
    assert_eq!(report["data"]["summary"]["value"]["eval_runs"], json!(1));
    assert_eq!(
        report["data"]["summary"]["value"]["status"],
        json!("available")
    );
    assert_eq!(report["data"]["summary"]["cost"]["tokens_in"], json!(123));
    assert_eq!(report["data"]["summary"]["cost"]["commands"], json!(2));
    assert_eq!(report["data"]["summary"]["risk"]["safety_events"], json!(1));
    assert_eq!(
        report["data"]["summary"]["risk"]["status"],
        json!("available")
    );

    let (inspect_output, inspect) = run_loom(
        root.path(),
        &["skill", "inspect", "demo", "--include-telemetry"],
    );
    assert!(
        inspect_output.status.success(),
        "inspect should pass: {inspect}"
    );
    assert_eq!(inspect["data"]["telemetry"]["enabled"], Value::Bool(true));
    assert_eq!(inspect["data"]["telemetry"]["events"], json!(2));
}

#[test]
fn telemetry_export_redacts_valid_events_and_quarantines_malformed_lines() {
    let root = TestDir::new("telemetry-export");
    let events_path = root.path().join("state/telemetry/events.jsonl");
    write_file(
        &events_path,
        &format!(
            "{}\n{{\"schema_version\":1,\"event_id\":\"evt_bad\",\"event_type\":\"skill.eval\",\"raw_prompt\":\"secret\"}}\n",
            event_line(
                "evt_export",
                "skill.eval",
                "demo",
                "2026-01-01T00:00:00Z",
                true
            )
        ),
    );

    let jsonl_out = root.path().join("export.jsonl");
    let jsonl_out_arg = jsonl_out.to_string_lossy().into_owned();
    let (jsonl_output, jsonl) = run_loom(
        root.path(),
        &[
            "telemetry",
            "export",
            "--format",
            "jsonl",
            "--output",
            &jsonl_out_arg,
        ],
    );
    assert!(
        jsonl_output.status.success(),
        "jsonl export should pass: {jsonl}"
    );
    assert_eq!(jsonl["data"]["events_exported"], json!(1));
    assert_eq!(jsonl["data"]["malformed_events_skipped"], json!(1));
    assert_eq!(jsonl["meta"]["warnings"].as_array().unwrap().len(), 1);
    let exported = fs::read_to_string(&jsonl_out).expect("read jsonl export");
    assert!(exported.contains(r#""event_id":"evt_export""#));
    assert!(!exported.contains("secret"));

    let csv_out = root.path().join("export.csv");
    let csv_out_arg = csv_out.to_string_lossy().into_owned();
    let (csv_output, csv) = run_loom(
        root.path(),
        &[
            "telemetry",
            "export",
            "--format",
            "csv",
            "--output",
            &csv_out_arg,
        ],
    );
    assert!(csv_output.status.success(), "csv export should pass: {csv}");
    let csv_body = fs::read_to_string(&csv_out).expect("read csv export");
    assert!(csv_body.starts_with("schema_version,event_id,event_type"));
    assert!(csv_body.contains("evt_export"));

    let state_output = root.path().join("state/telemetry/unsafe.jsonl");
    let state_output_arg = state_output.to_string_lossy().into_owned();
    let (blocked_output, blocked) = run_loom(
        root.path(),
        &[
            "telemetry",
            "export",
            "--format",
            "jsonl",
            "--output",
            &state_output_arg,
        ],
    );
    assert!(
        !blocked_output.status.success(),
        "unsafe export should fail"
    );
    assert_eq!(blocked["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn telemetry_purge_dry_run_and_confirm_only_remove_matching_events() {
    let root = TestDir::new("telemetry-purge");
    let events_path = root.path().join("state/telemetry/events.jsonl");
    write_file(
        &events_path,
        &format!(
            "{}\n{}\nnot-json\n",
            event_line(
                "evt_old",
                "skill.eval",
                "demo",
                "2026-01-01T00:00:00Z",
                true
            ),
            event_line(
                "evt_new",
                "skill.eval",
                "demo",
                "2026-02-01T00:00:00Z",
                true
            )
        ),
    );

    let (dry_output, dry) = run_loom(
        root.path(),
        &["telemetry", "purge", "--before", "2026-01-15", "--dry-run"],
    );
    assert!(dry_output.status.success(), "dry-run should pass: {dry}");
    assert_eq!(dry["data"]["matching_events"], json!(1));
    assert_eq!(dry["data"]["malformed_events_preserved"], json!(1));
    assert!(
        fs::read_to_string(&events_path)
            .expect("read events")
            .contains("evt_old"),
        "dry-run must not mutate"
    );
    let token = dry["data"]["confirm_token"]
        .as_str()
        .expect("confirm token")
        .to_string();

    let (confirm_output, confirm) = run_loom(
        root.path(),
        &[
            "telemetry",
            "purge",
            "--before",
            "2026-01-15",
            "--confirm",
            &token,
        ],
    );
    assert!(
        confirm_output.status.success(),
        "confirmed purge should pass: {confirm}"
    );
    assert_eq!(confirm["data"]["deleted_events"], json!(1));
    let remaining = fs::read_to_string(&events_path).expect("read purged events");
    assert!(!remaining.contains("evt_old"));
    assert!(remaining.contains("evt_new"));
    assert!(remaining.contains("not-json"));
}
