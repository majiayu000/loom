use std::fs;

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom, write_minimal_registry_state, write_skill};

#[test]
fn skill_used_absent_telemetry_reports_not_recorded_without_state() {
    let root = TestDir::new("telemetry-skill-used-absent");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing usage telemetry.\n---\n# Demo\n",
    );

    let (used_output, used) = run_loom(
        root.path(),
        &[
            "skill",
            "used",
            "demo",
            "--agent",
            "codex",
            "--tokens-in",
            "7",
        ],
    );

    assert!(
        used_output.status.success(),
        "skill used should pass: {used}"
    );
    assert_eq!(used["cmd"], json!("skill.used"));
    assert_eq!(used["data"]["event_type"], json!("skill.invocation"));
    assert_eq!(used["data"]["recorded"], json!(false));
    assert_eq!(used["data"]["reason"], json!("telemetry_disabled"));
    assert_eq!(used["data"]["event_id"], Value::Null);
    assert!(
        !root.path().join("state/telemetry").exists(),
        "absent telemetry config must remain blank"
    );
}

#[test]
fn skill_used_accepts_registry_read_model_skill_without_source_dir() {
    let root = TestDir::new("telemetry-skill-used-read-model");
    write_minimal_registry_state(root.path(), 1);

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );

    let (used_output, used) = run_loom(
        root.path(),
        &["skill", "used", "model-onboarding", "--agent", "claude"],
    );

    assert!(
        used_output.status.success(),
        "read-model skill telemetry should pass without source dir: {used}"
    );
    assert_eq!(used["data"]["recorded"], json!(true));
    let raw_events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl"))
        .expect("read telemetry events");
    assert!(raw_events.contains(r#""skill_id":"model-onboarding""#));
}

#[test]
fn skill_used_and_feedback_write_redacted_events_and_reports() {
    let root = TestDir::new("telemetry-skill-used-feedback");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing usage telemetry.\n---\n# Demo\n",
    );

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );

    let (empty_report_output, empty_report) =
        run_loom(root.path(), &["telemetry", "report", "--skill", "demo"]);
    assert!(
        empty_report_output.status.success(),
        "empty report should pass: {empty_report}"
    );
    assert_eq!(
        empty_report["data"]["instrumentation"]["skill.invocation"]["status"],
        json!("instrumented")
    );
    assert_eq!(
        empty_report["data"]["instrumentation"]["telemetry.sync"]["status"],
        json!("not_instrumented")
    );
    assert_eq!(
        empty_report["data"]["summary"]["usage"]["status"],
        json!("missing")
    );
    assert_eq!(
        empty_report["data"]["summary"]["sync"]["status"],
        json!("not_instrumented")
    );
    assert_eq!(
        empty_report["data"]["summary"]["recommendation_feedback"]["status"],
        json!("missing")
    );

    let workspace_arg = root.path().to_string_lossy().into_owned();
    let (used_output, used) = run_loom(
        root.path(),
        &[
            "skill",
            "used",
            "demo",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
            "--session-id",
            "session-secret",
            "--tokens-in",
            "11",
            "--tokens-out",
            "3",
            "--commands",
            "2",
            "--duration-ms",
            "150",
        ],
    );
    assert!(
        used_output.status.success(),
        "skill used should pass: {used}"
    );
    assert_eq!(used["data"]["recorded"], json!(true));
    assert_eq!(used["data"]["event_type"], json!("skill.invocation"));
    assert!(used["data"]["event_id"].as_str().is_some());

    let (error_output, error) = run_loom(
        root.path(),
        &[
            "skill",
            "used",
            "demo",
            "--agent",
            "codex",
            "--error",
            "--failure-category",
            "timeout",
        ],
    );
    assert!(
        error_output.status.success(),
        "skill used --error should pass: {error}"
    );
    assert_eq!(error["data"]["event_type"], json!("skill.error"));
    assert_eq!(error["data"]["failure_category"], json!("timeout"));

    let (feedback_output, feedback) = run_loom(
        root.path(),
        &[
            "skill",
            "feedback",
            "demo",
            "--feedback",
            "accepted",
            "--agent",
            "codex",
            "--task",
            "do not persist raw task sk_test_secret",
            "--session-id",
            "feedback-session-secret",
        ],
    );
    assert!(
        feedback_output.status.success(),
        "skill feedback should pass: {feedback}"
    );
    assert_eq!(
        feedback["data"]["event_type"],
        json!("recommendation.feedback")
    );
    assert_eq!(feedback["data"]["feedback"], json!("accepted"));

    let raw_events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl"))
        .expect("read telemetry events");
    assert!(raw_events.contains(r#""event_type":"skill.invocation""#));
    assert!(raw_events.contains(r#""event_type":"skill.error""#));
    assert!(raw_events.contains(r#""event_type":"recommendation.feedback""#));
    assert!(raw_events.contains(r#""schema_version":2"#));
    assert!(raw_events.contains(r#""failure_category":"timeout""#));
    assert!(raw_events.contains(r#""feedback":"accepted""#));
    assert!(raw_events.contains(r#""task_hash":"sha256:"#));
    assert!(!raw_events.contains("sk_test_secret"));
    assert!(!raw_events.contains("raw task"));
    assert!(!raw_events.contains("session-secret"));

    let csv_out = root.path().join("usage-export.csv");
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
    assert!(csv_body.contains("failure_category"));
    assert!(csv_body.contains("task_hash"));
    assert!(csv_body.contains("timeout"));

    let (report_output, report) =
        run_loom(root.path(), &["telemetry", "report", "--skill", "demo"]);
    assert!(
        report_output.status.success(),
        "report should pass: {report}"
    );
    assert_eq!(report["data"]["matched_events"], json!(3));
    assert_eq!(report["data"]["summary"]["usage"]["invocations"], json!(1));
    assert_eq!(report["data"]["summary"]["usage"]["errors"], json!(1));
    assert_eq!(
        report["data"]["summary"]["usage"]["status"],
        json!("available")
    );
    assert_eq!(
        report["data"]["summary"]["recommendation_feedback"]["accepted"],
        json!(1)
    );
    assert_eq!(
        report["data"]["summary"]["recommendation_feedback"]["status"],
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
    assert_eq!(
        inspect["data"]["telemetry"]["usage"]["invocations"],
        json!(1)
    );
    assert_eq!(inspect["data"]["telemetry"]["usage"]["errors"], json!(1));
    assert!(
        inspect["data"]["telemetry"]["usage"]["last_error_at"]
            .as_str()
            .is_some(),
        "inspect telemetry should expose last error timestamp: {inspect}"
    );
    assert_eq!(
        inspect["data"]["telemetry"]["recommendation_feedback"]["accepted"],
        json!(1)
    );
}

#[test]
fn skill_used_error_requires_structured_failure_category() {
    let root = TestDir::new("telemetry-skill-used-error-category");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing usage telemetry.\n---\n# Demo\n",
    );

    let (missing_output, missing) = run_loom(root.path(), &["skill", "used", "demo", "--error"]);
    assert!(!missing_output.status.success(), "{missing}");
    assert_eq!(missing["error"]["code"], json!("ARG_INVALID"));

    let (raw_output, raw) = run_loom(
        root.path(),
        &[
            "skill",
            "used",
            "demo",
            "--error",
            "--failure-category",
            "raw timeout text",
        ],
    );
    assert!(!raw_output.status.success(), "{raw}");
    assert_eq!(raw["error"]["code"], json!("ARG_INVALID"));

    let (token_output, token) = run_loom(
        root.path(),
        &[
            "skill",
            "used",
            "demo",
            "--error",
            "--failure-category",
            "sk_test_secret",
        ],
    );
    assert!(!token_output.status.success(), "{token}");
    assert_eq!(token["error"]["code"], json!("ARG_INVALID"));
}
