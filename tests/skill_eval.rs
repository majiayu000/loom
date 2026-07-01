mod common;

use std::fs;
use std::process::Command;

use serde_json::json;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};

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

#[test]
fn skill_eval_run_dry_run_returns_plan_without_writes() {
    let root = TestDir::new("skill-eval-run-dry");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"task-1","prompt":"Diagnose the failing tests","checks":{"outcome_contains":["tests pass"],"commands_contains":["cargo test"],"exit_code":0}}
"#,
    );

    let (output, env) = run_loom(
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
            "--dry-run",
        ],
    );

    assert!(
        output.status.success(),
        "dry-run should pass: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["data"]["mode"], json!("real_agent_baseline"));
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(env["data"]["plan"]["will_write_report"], json!(false));
    assert_eq!(
        env["data"]["plan"]["resolved_cases"][0]["id"],
        json!("task-1")
    );
    assert!(!root.path().join("state/registry/evals").exists());
}

#[test]
fn skill_eval_run_mock_baseline_persists_report_and_redacts_prompts() {
    let root = TestDir::new("skill-eval-run-mock");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        r#"{"id":"t1","prompt":"use demo for this","should_trigger":true,"observed_trigger":true}
{"id":"t2","prompt":"summarize this","should_trigger":false,"observed_trigger":false}
"#,
    );
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"task-1","prompt":"Diagnose confidential-eval-marker","checks":{"exit_code":0,"files_changed":["src/foo.rs"],"commands_contains":["cargo test"],"outcome_contains":["tests pass"],"max_tokens":200,"max_commands":3}}
"#,
    );
    let report_path = root.path().join("reports/run.json");
    let report_arg = report_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
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
            "--runner",
            "mock",
            "--output",
            report_arg.as_str(),
        ],
    );

    assert!(
        output.status.success(),
        "mock baseline should pass: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["data"]["summary"]["with_skill_pass_rate"], json!(1.0));
    assert_eq!(
        env["data"]["summary"]["without_skill_pass_rate"],
        json!(0.0)
    );
    assert_eq!(env["data"]["summary"]["delta"], json!(1.0));
    assert_eq!(env["data"]["summary"]["trigger_precision"], json!(1.0));
    assert_eq!(env["data"]["summary"]["trigger_recall"], json!(1.0));
    assert_eq!(env["data"]["report_path"], json!(report_arg));
    let raw_report = fs::read_to_string(report_path).expect("read report");
    assert!(!raw_report.contains("confidential-eval-marker"));
}

#[test]
fn skill_eval_trigger_reports_precision_recall() {
    let root = TestDir::new("skill-eval-trigger");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        r#"{"id":"positive","prompt":"use demo","should_trigger":true}
{"id":"negative","prompt":"read a memo","should_trigger":false,"observed_trigger":false}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "eval", "trigger", "demo", "--agent", "codex"],
    );

    assert!(
        output.status.success(),
        "trigger eval should pass: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["data"]["mode"], json!("trigger_quality"));
    assert_eq!(env["data"]["summary"]["trigger_precision"], json!(1.0));
    assert_eq!(env["data"]["summary"]["trigger_recall"], json!(1.0));
    assert!(
        root.path()
            .join("state/registry/evals/demo/trigger-latest.json")
            .is_file()
    );
}

#[test]
fn skill_eval_codex_cli_runner_missing_executable_is_typed() {
    let root = TestDir::new("skill-eval-codex-missing");
    write_skill(root.path(), "demo", "# Demo\n");

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("PATH", "")],
        &[
            "skill",
            "eval",
            "run",
            "demo",
            "--agent",
            "codex",
            "--baseline",
            "no-skill",
            "--runner",
            "codex-cli",
        ],
    );

    assert!(!output.status.success(), "missing codex runner must fail");
    assert_eq!(env["error"]["code"], json!("EVAL_FAILED"));
    assert_eq!(
        env["error"]["details"]["failure_kind"],
        json!("runner_executable_missing")
    );
}

#[test]
fn skill_eval_cleanup_failure_keeps_report_details() {
    let root = TestDir::new("skill-eval-cleanup-failure");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"task-1","prompt":"Fix tests","checks":{"outcome_contains":["tests pass"]}}
"#,
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_EVAL_MOCK_CLEANUP_FAIL", "1")],
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
        !output.status.success(),
        "cleanup failure must fail command"
    );
    assert_eq!(env["error"]["code"], json!("EVAL_FAILED"));
    assert_eq!(
        env["error"]["details"]["report"]["cleanup"]["status"],
        json!("failed")
    );
    assert_eq!(
        env["error"]["details"]["report"]["summary"]["with_skill_pass_rate"],
        json!(1.0)
    );
}

#[test]
fn skill_eval_compare_evaluates_refs_without_mutating_source() {
    let root = TestDir::new("skill-eval-compare");
    write_skill(root.path(), "demo", "# Demo\n");
    write_file(
        &root.path().join("skills/demo/evals/tasks.jsonl"),
        r#"{"id":"task-1","prompt":"Fix tests","checks":{"outcome_contains":["tests pass"]}}
"#,
    );
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["add", "skills"]);
    git(
        root.path(),
        &[
            "-c",
            "user.name=loom",
            "-c",
            "user.email=loom@example.invalid",
            "commit",
            "-m",
            "seed skill",
        ],
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "eval",
            "compare",
            "demo",
            "--from",
            "HEAD",
            "--to",
            "working-tree",
            "--agent",
            "codex",
        ],
    );

    assert!(
        output.status.success(),
        "compare should pass: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["data"]["mode"], json!("version_compare"));
    assert_eq!(env["data"]["summary"]["from_pass_rate"], json!(1.0));
    assert_eq!(env["data"]["summary"]["to_pass_rate"], json!(1.0));
    assert!(env["data"]["from"]["skill_version"]["head_tree_oid"].is_string());
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
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
}
