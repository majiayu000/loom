mod common;

use serde_json::json;

use common::{TestDir, run_loom, write_file, write_skill};

#[test]
fn skill_eval_runs_offline_fixture_matrix() {
    let root = TestDir::new("skill-eval-matrix");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing Loom skill eval fixtures.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        r#"{"id":"positive","prompt":"Use demo for this task","expected_trigger":true,"observed_trigger":true}
{"id":"negative","prompt":"Summarize a sales email","expected_trigger":false,"observed_trigger":false}
"#,
    );
    write_file(
        &root.path().join("skills/demo/evals/artifacts/result.txt"),
        "offline artifact\n",
    );
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"happy-path","task":"Run the demo eval","output":"Done with concise result","trace":["read SKILL.md","checked artifact"],"metrics":{"tokens":42,"commands":2},"permissions_used":["filesystem:read"],"checks":{"outcome_contains":["Done"],"process_contains":["SKILL.md"],"style_contains":["concise"],"max_tokens":100,"max_commands":3,"artifacts":[{"path":"evals/artifacts/result.txt"}]}}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "eval",
            "demo",
            "--matrix",
            "claude,codex",
            "--model",
            "fixture-model",
        ],
    );

    assert!(
        output.status.success(),
        "eval should pass: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["cmd"], json!("skill.eval"));
    assert_eq!(env["data"]["matrix"], json!(["claude", "codex"]));
    assert_eq!(env["data"]["summary"]["case_count"], json!(6));
    assert_eq!(env["data"]["summary"]["passed"], json!(6));
    assert_eq!(env["data"]["summary"]["aggregate_score"], json!(1.0));
    assert_eq!(env["data"]["summary"]["trigger_precision"], json!(1.0));
    assert_eq!(env["data"]["summary"]["trigger_recall"], json!(1.0));
    assert_eq!(env["data"]["summary"]["task_success_rate"], json!(1.0));
    assert_eq!(
        env["data"]["summary"]["permissions_used"],
        json!(["filesystem:read"])
    );
    assert_eq!(
        env["data"]["runs"][0]["tasks"][0]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["id"] == json!("artifacts"))
            .expect("artifact check")["status"],
        json!("passed")
    );
    assert_eq!(
        env["data"]["security_model"]["eval_success_is_safety_guarantee"],
        json!(false)
    );
}

#[test]
fn skill_eval_reports_missing_fixtures_as_blank_with_warning() {
    let root = TestDir::new("skill-eval-empty");
    write_skill(root.path(), "demo", "# Demo\n");

    let (output, env) = run_loom(root.path(), &["skill", "eval", "demo"]);

    assert!(
        output.status.success(),
        "missing eval files should not fail"
    );
    assert_eq!(env["data"]["summary"]["case_count"], json!(0));
    assert_eq!(env["data"]["summary"]["aggregate_score"], json!(null));
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap_or("")
                .contains("no eval cases found"))
    );
}

#[test]
fn skill_eval_fails_closed_when_cases_fail() {
    let root = TestDir::new("skill-eval-case-failure");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        r#"{"id":"regression","prompt":"Use demo here","expected_trigger":true,"observed_trigger":false}
"#,
    );

    let (output, env) = run_loom(root.path(), &["skill", "eval", "demo"]);

    assert!(!output.status.success(), "failing eval cases must fail");
    assert_eq!(env["error"]["code"], json!("EVAL_FAILED"));
    assert_eq!(env["error"]["details"]["failed"], json!(1));
    assert_eq!(
        env["error"]["details"]["report"]["summary"]["failed"],
        json!(1)
    );
    assert_eq!(
        env["error"]["details"]["report"]["runs"][0]["triggers"][0]["status"],
        json!("failed")
    );
}

#[test]
fn skill_eval_rejects_invalid_fixture_json() {
    let root = TestDir::new("skill-eval-invalid-json");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        "{\"id\":\"bad\",\"expected_trigger\":true\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "eval", "demo"]);

    assert!(
        !output.status.success(),
        "invalid eval JSON should fail closed"
    );
    assert_eq!(env["error"]["code"], json!("SCHEMA_MISMATCH"));
    assert_eq!(env["error"]["details"]["line"], json!(1));
}
