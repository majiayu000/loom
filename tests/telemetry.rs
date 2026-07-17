use std::fs;

use serde_json::{Value, json};

mod common;
#[path = "telemetry/overflow.rs"]
mod telemetry_overflow;

use common::{TestDir, run_loom, run_loom_in_cwd, write_file, write_skill};

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
fn telemetry_export_reads_v2_and_redacts_v3_observed_name() {
    let root = TestDir::new("telemetry-export-v2-v3");
    let events_path = root.path().join("state/telemetry/events.jsonl");
    let v2 = json!({
        "schema_version": 2,
        "event_id": "evt_v2",
        "event_type": "skill.invocation",
        "skill_id": "demo",
        "timestamp": "2026-07-01T00:00:00Z",
        "metrics": {},
        "privacy": {"raw_prompt_stored": false, "raw_code_stored": false, "redacted": true}
    });
    let v3 = json!({
        "schema_version": 3,
        "event_id": "evt_v3",
        "event_type": "skill.invocation",
        "observed_skill_name": "retired-skill",
        "timestamp": "2026-07-01T00:01:00Z",
        "metrics": {},
        "privacy": {"raw_prompt_stored": false, "raw_code_stored": false, "redacted": true}
    });
    write_file(&events_path, &format!("{v2}\n{v3}\n"));
    let output_path = root.path().join("events.csv");
    let output_arg = output_path.to_string_lossy().into_owned();
    let (output, envelope) = run_loom(
        root.path(),
        &[
            "telemetry",
            "export",
            "--format",
            "csv",
            "--output",
            &output_arg,
        ],
    );
    assert!(output.status.success(), "export failed: {envelope}");
    assert_eq!(envelope["data"]["events_exported"], json!(2));
    let csv = fs::read_to_string(output_path).unwrap();
    assert!(csv.lines().next().unwrap().contains("observed_skill_name"));
    assert!(csv.contains("retired-skill"));
}

#[test]
fn normalized_query_preserves_report_metrics_and_filters() {
    let root = TestDir::new("telemetry-normalized-query");
    let events_path = root.path().join("state/telemetry/events.jsonl");
    let base = |id: &str, event_type: &str, skillset: &str, agent: Option<&str>, metrics: Value| {
        let mut event = json!({
            "schema_version": 2,
            "event_id": id,
            "event_type": event_type,
            "skill_id": "demo",
            "skillset_id": skillset,
            "workspace_hash": "sha256:workspace",
            "timestamp": "2026-07-01T00:00:00Z",
            "metrics": metrics,
            "privacy": {
                "raw_prompt_stored": false,
                "raw_code_stored": false,
                "redacted": true
            }
        });
        if let Some(agent) = agent {
            event["agent"] = json!(agent);
        }
        event
    };
    let activation = base(
        "evt_activation",
        "skill.activation",
        "pack",
        None,
        json!({"tokens_in": 5}),
    );
    let eval = base(
        "evt_eval",
        "skill.eval",
        "pack",
        Some("codex"),
        json!({"tokens_in": 7, "success": true, "baseline_delta": 0.5}),
    );
    let feedback = base(
        "evt_feedback",
        "recommendation.feedback",
        "other",
        Some("codex"),
        json!({"feedback": "accepted"}),
    );
    write_file(&events_path, &format!("{activation}\n{eval}\n{feedback}\n"));
    let (output, report) = run_loom(
        root.path(),
        &[
            "telemetry",
            "report",
            "--skill",
            "demo",
            "--skillset",
            "pack",
        ],
    );
    assert!(output.status.success(), "report failed: {report}");
    assert_eq!(report["data"]["matched_events"], json!(2));
    assert_eq!(report["data"]["summary"]["usage"]["activations"], json!(1));
    assert_eq!(report["data"]["summary"]["value"]["eval_runs"], json!(1));
    assert_eq!(report["data"]["summary"]["cost"]["tokens_in"], json!(12));
    assert_eq!(
        report["data"]["summary"]["recommendation_feedback"]["status"],
        json!("missing")
    );
    let (agent_output, agent_report) = run_loom(
        root.path(),
        &[
            "telemetry",
            "report",
            "--skillset",
            "pack",
            "--agent",
            "codex",
        ],
    );
    assert!(
        agent_output.status.success(),
        "agent report failed: {agent_report}"
    );
    assert_eq!(agent_report["data"]["matched_events"], json!(1));
    assert_eq!(
        agent_report["data"]["summary"]["usage"]["activations"],
        json!(0)
    );
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
    let gitignore = fs::read_to_string(root.path().join(".gitignore")).expect("read .gitignore");
    assert!(
        gitignore.lines().any(|line| line == "state/telemetry/"),
        "local telemetry state must stay out of registry Git status"
    );

    let (disable_output, disable) = run_loom(root.path(), &["telemetry", "disable"]);
    assert!(
        disable_output.status.success(),
        "disable should pass: {disable}"
    );
    assert_eq!(disable["data"]["enabled"], Value::Bool(false));

    let (scan_output, scan) = run_loom(root.path(), &["skill", "scan", "demo"]);
    assert!(scan_output.status.success(), "scan should pass: {scan}");
    let (used_output, used) = run_loom(root.path(), &["skill", "used", "demo"]);
    assert!(
        used_output.status.success(),
        "skill used should pass while telemetry is disabled: {used}"
    );
    assert_eq!(used["data"]["recorded"], json!(false));
    let (feedback_output, feedback) = run_loom(
        root.path(),
        &["skill", "feedback", "demo", "--feedback", "ignored"],
    );
    assert!(
        feedback_output.status.success(),
        "skill feedback should pass while telemetry is disabled: {feedback}"
    );
    assert_eq!(feedback["data"]["recorded"], json!(false));
    assert!(
        !root.path().join("state/telemetry/events.jsonl").exists(),
        "disabled telemetry must not append events"
    );
}

#[test]
fn telemetry_absent_config_does_not_initialize_event_state() {
    let root = TestDir::new("telemetry-absent-config-no-state");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing absent telemetry config.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"case-1","task":"do work","output":"ok"}"#,
    );

    let (eval_output, eval) = run_loom(root.path(), &["skill", "eval", "demo"]);
    assert!(
        eval_output.status.success(),
        "eval without telemetry config should pass: {eval}"
    );
    assert!(
        !root.path().join("state/telemetry").exists(),
        "absent telemetry config must remain blank and not initialize event state"
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

    let (eval_output, eval) = run_loom(root.path(), &["skill", "eval", "demo", "--agent", "codex"]);
    assert!(eval_output.status.success(), "eval should pass: {eval}");
    let (scan_output, scan) = run_loom(root.path(), &["skill", "scan", "demo"]);
    assert!(scan_output.status.success(), "scan should pass: {scan}");

    let raw_events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl"))
        .expect("read telemetry events");
    assert!(raw_events.contains(r#""event_type":"skill.eval""#));
    assert!(raw_events.contains(r#""event_type":"skill.safety""#));
    assert!(raw_events.contains(r#""agent":"codex""#));
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
    assert_eq!(
        report["data"]["panel_read_model"]["deferred_ui"],
        json!(false)
    );

    let (agent_report_output, agent_report) = run_loom(
        root.path(),
        &["telemetry", "report", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        agent_report_output.status.success(),
        "agent report should pass: {agent_report}"
    );
    assert_eq!(agent_report["data"]["matched_events"], json!(1));
    assert_eq!(
        agent_report["data"]["summary"]["cost"]["status"],
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
    let raw_failure_category = json!({
        "schema_version": 1,
        "event_id": "evt_raw_error",
        "event_type": "skill.error",
        "skill_id": "demo",
        "timestamp": "2026-01-01T00:00:00Z",
        "metrics": {
            "success": false,
            "failure_category": "raw stack trace secret"
        },
        "privacy": {
            "raw_prompt_stored": false,
            "raw_code_stored": false,
            "redacted": true
        }
    });
    write_file(
        &events_path,
        &format!(
            "{}\n{}\n{{\"schema_version\":1,\"event_id\":\"evt_bad\",\"event_type\":\"skill.eval\",\"raw_prompt\":\"secret\"}}\n",
            event_line(
                "evt_export",
                "skill.eval",
                "demo",
                "2026-01-01T00:00:00Z",
                true
            ),
            raw_failure_category
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
    assert_eq!(jsonl["data"]["malformed_events_skipped"], json!(2));
    assert_eq!(jsonl["meta"]["warnings"].as_array().unwrap().len(), 2);
    assert!(
        jsonl["meta"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("failure_category")))
    );
    let exported = fs::read_to_string(&jsonl_out).expect("read jsonl export");
    assert!(exported.contains(r#""event_id":"evt_export""#));
    assert!(!exported.contains("secret"));
    assert!(!exported.contains("raw stack trace"));

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

    #[cfg(unix)]
    {
        let symlink_output = root.path().join("exports");
        std::os::unix::fs::symlink(root.path().join("state/telemetry"), &symlink_output)
            .expect("create export symlink");
        let symlink_output_arg = symlink_output
            .join("config.json")
            .to_string_lossy()
            .into_owned();
        let (symlink_blocked_output, symlink_blocked) = run_loom(
            root.path(),
            &[
                "telemetry",
                "export",
                "--format",
                "jsonl",
                "--output",
                &symlink_output_arg,
            ],
        );
        assert!(
            !symlink_blocked_output.status.success(),
            "symlinked export into state should fail"
        );
        assert_eq!(symlink_blocked["error"]["code"], json!("POLICY_BLOCKED"));
    }
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

#[test]
fn telemetry_purge_rejects_stale_confirm_token_after_matching_append() {
    let root = TestDir::new("telemetry-purge-stale-token");
    let events_path = root.path().join("state/telemetry/events.jsonl");
    write_file(
        &events_path,
        &format!(
            "{}\n{}\n",
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
    let token = dry["data"]["confirm_token"]
        .as_str()
        .expect("confirm token")
        .to_string();
    write_file(
        &events_path,
        &format!(
            "{}{}",
            fs::read_to_string(&events_path).expect("read events"),
            event_line(
                "evt_added",
                "skill.eval",
                "demo",
                "2026-01-02T00:00:00Z",
                true
            )
        ),
    );

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
        !confirm_output.status.success(),
        "stale confirm token should fail"
    );
    assert_eq!(confirm["error"]["code"], json!("ARG_INVALID"));
    let remaining = fs::read_to_string(&events_path).expect("read events after failed purge");
    assert!(remaining.contains("evt_old"));
    assert!(remaining.contains("evt_added"));
}

#[test]
fn telemetry_activation_uses_project_workspace_for_report_filter() {
    let root = TestDir::new("telemetry-activation-workspace");
    let workspace = TestDir::new("telemetry-activation-workspace-project");
    let caller_cwd = TestDir::new("telemetry-activation-workspace-cwd");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing telemetry activation.\n---\n# Demo\n",
    );
    let (_, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert_eq!(enable["ok"], Value::Bool(true));

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (activate_output, activate) = run_loom_in_cwd(
        root.path(),
        caller_cwd.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--scope",
            "project",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(
        activate_output.status.success(),
        "project activation should pass: {activate}"
    );

    let (workspace_report_output, workspace_report) = run_loom(
        root.path(),
        &["telemetry", "report", "--workspace", &workspace_arg],
    );
    assert!(
        workspace_report_output.status.success(),
        "workspace report should pass: {workspace_report}"
    );
    assert_eq!(workspace_report["data"]["matched_events"], json!(1));

    let caller_arg = caller_cwd.path().to_string_lossy().to_string();
    let (caller_report_output, caller_report) = run_loom(
        root.path(),
        &["telemetry", "report", "--workspace", &caller_arg],
    );
    assert!(
        caller_report_output.status.success(),
        "caller cwd report should pass: {caller_report}"
    );
    assert_eq!(caller_report["data"]["matched_events"], json!(0));
}

#[test]
fn telemetry_activation_warning_does_not_fail_committed_activation() {
    let root = TestDir::new("telemetry-activation-warning");
    let workspace = TestDir::new("telemetry-activation-warning-project");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing telemetry activation warning.\n---\n# Demo\n",
    );
    let (_, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert_eq!(enable["ok"], Value::Bool(true));
    write_file(
        &root.path().join("state/telemetry/config.json"),
        "not-json\n",
    );

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (activate_output, activate) = run_loom(
        root.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--scope",
            "project",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(
        activate_output.status.success(),
        "committed activation should not fail on telemetry append error: {activate}"
    );
    assert_eq!(activate["data"]["noop"], Value::Bool(false));
    let warnings = activate["meta"]["warnings"].as_array().expect("warnings");
    assert!(warnings.iter().any(|warning| {
        warning
            .as_str()
            .unwrap_or_default()
            .contains("SCHEMA_MISMATCH")
    }));
    assert!(
        workspace.path().join(".agents/skills/demo").exists(),
        "activation projection should be committed"
    );
}

#[test]
fn telemetry_harness_eval_keeps_missing_cost_evidence() {
    let root = TestDir::new("telemetry-harness-cost");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing telemetry harness cost.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        "{\"id\":\"case-1\",\"input\":\"do task\"}\n",
    );
    let (_, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert_eq!(enable["ok"], Value::Bool(true));

    let (run_output, run) = run_loom(
        root.path(),
        &[
            "skill",
            "eval",
            "run",
            "demo",
            "--agent",
            "codex",
            "--baseline",
            "no-skill",
        ],
    );
    assert!(
        run_output.status.success(),
        "harness eval run should pass: {run}"
    );

    let raw_events = fs::read_to_string(root.path().join("state/telemetry/events.jsonl"))
        .expect("read telemetry events");
    assert!(raw_events.contains(r#""event_type":"skill.eval""#));
    assert!(raw_events.contains(r#""agent":"codex""#));
    assert!(!raw_events.contains("tokens_in"));
    assert!(!raw_events.contains(r#""commands""#));

    let (report_output, report) = run_loom(
        root.path(),
        &["telemetry", "report", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        report_output.status.success(),
        "report should pass: {report}"
    );
    assert_eq!(report["data"]["matched_events"], json!(1));
    assert_eq!(
        report["data"]["summary"]["cost"]["status"],
        json!("missing")
    );
}
