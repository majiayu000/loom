mod common;

use std::fs;

use serde_json::{Value, json};

use common::{TestDir, run_loom, write_file, write_skill};

fn write_recommend_skill(root: &std::path::Path, skill: &str, description: &str) {
    write_skill(
        root,
        skill,
        &format!("---\nname: {skill}\ndescription: {description}\n---\n# {skill}\n"),
    );
}

fn recommendation_results(env: &Value) -> &[Value] {
    env["data"]["recommendations"]["results"]
        .as_array()
        .expect("recommendation results")
}

fn recommendation<'a>(env: &'a Value, id: &str) -> &'a Value {
    recommendation_results(env)
        .iter()
        .find(|result| result["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("missing recommendation {id}: {env}"))
}

fn telemetry_line(
    id: &str,
    event_type: &str,
    skill: &str,
    timestamp: &str,
    metrics: Value,
) -> String {
    json!({
        "schema_version": 1,
        "event_id": id,
        "event_type": event_type,
        "skill_id": skill,
        "agent": "codex",
        "timestamp": timestamp,
        "metrics": metrics,
        "privacy": {
            "raw_prompt_stored": false,
            "raw_code_stored": false,
            "redacted": true
        }
    })
    .to_string()
}

#[test]
fn invalid_skill_ids_do_not_abort_index_or_recommendations() {
    let root = TestDir::new("recommend-invalid-skill-id");
    write_file(
        &root.path().join("skills/bad name/SKILL.md"),
        "---\nname: bad name\ndescription: Use when testing invalid skill ids.\n---\n# Bad\n",
    );

    let (output, env) = run_loom(root.path(), &["index", "build", "--no-embeddings"]);
    assert!(output.status.success(), "index build should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "recommend",
            "invalid skill ids",
            "--agent",
            "codex",
        ],
    );
    assert!(
        output.status.success(),
        "recommend should carry invalid inventory records without aborting: {env}"
    );
}

#[test]
fn recommend_filters_eval_evidence_by_requested_agent() {
    let root = TestDir::new("recommend-agent-eval-filter");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }
    write_file(
        &root
            .path()
            .join("state/registry/evals/z-ci-helper/run-latest.json"),
        r#"{"schema_version":1,"skill":"z-ci-helper","agent":"claude","summary":{"delta":0.8,"failed":0}}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(output.status.success(), "recommend should pass: {env}");
    assert_eq!(recommendation_results(&env)[0]["id"], json!("a-ci-helper"));
    let mismatched = recommendation(&env, "z-ci-helper");
    assert!(
        mismatched["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning.as_str() == Some("no eval evidence")),
        "mismatched-agent eval should not count as persisted evidence: {mismatched}"
    );
}

#[test]
fn recommend_penalizes_negative_eval_delta() {
    let root = TestDir::new("recommend-negative-delta");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }
    write_file(
        &root
            .path()
            .join("state/registry/evals/z-ci-helper/compare-latest.json"),
        r#"{"schema_version":1,"skill":"z-ci-helper","agent":"codex","mode":"version_compare","summary":{"delta":-0.6,"failed":0}}
"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(output.status.success(), "recommend should pass: {env}");
    let baseline = recommendation(&env, "a-ci-helper");
    let regressed = recommendation(&env, "z-ci-helper");
    assert!(
        regressed["score"].as_i64().unwrap() < baseline["score"].as_i64().unwrap(),
        "negative delta should reduce ranking: {env}"
    );
    assert!(
        regressed["risks"]
            .as_array()
            .expect("risks")
            .iter()
            .any(|risk| risk
                .as_str()
                .is_some_and(|value| value.contains("eval baseline delta"))),
        "negative delta should be explicit risk: {regressed}"
    );
}

#[test]
fn recommend_uses_telemetry_usage_and_feedback_when_present() {
    let root = TestDir::new("recommend-telemetry-evidence");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }

    let (baseline_output, baseline) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );
    assert!(
        baseline_output.status.success(),
        "baseline recommend should pass: {baseline}"
    );
    assert_eq!(
        recommendation_results(&baseline)[0]["id"],
        json!("a-ci-helper")
    );

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );
    let (used_output, used) = run_loom(
        root.path(),
        &["skill", "used", "z-ci-helper", "--agent", "codex"],
    );
    assert!(
        used_output.status.success(),
        "skill used should pass: {used}"
    );
    let (feedback_output, feedback) = run_loom(
        root.path(),
        &[
            "skill",
            "feedback",
            "z-ci-helper",
            "--feedback",
            "accepted",
            "--agent",
            "codex",
            "--task",
            "fix failing ci",
        ],
    );
    assert!(
        feedback_output.status.success(),
        "skill feedback should pass: {feedback}"
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(output.status.success(), "recommend should pass: {env}");
    assert_eq!(recommendation_results(&env)[0]["id"], json!("z-ci-helper"));
    let boosted = recommendation(&env, "z-ci-helper");
    assert!(
        boosted["score_inputs"]
            .as_array()
            .expect("score inputs")
            .iter()
            .any(|input| input["field"] == json!("telemetry_usage")),
        "usage telemetry should appear in score inputs: {boosted}"
    );
    assert!(
        boosted["score_inputs"]
            .as_array()
            .expect("score inputs")
            .iter()
            .any(|input| input["field"] == json!("recommendation_feedback")
                && input["metric"] == json!("accepted")),
        "feedback telemetry should appear in score inputs: {boosted}"
    );
}

#[test]
fn recommend_scopes_feedback_to_requested_task() {
    let root = TestDir::new("recommend-task-scoped-feedback");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );
    let (feedback_output, feedback) = run_loom(
        root.path(),
        &[
            "skill",
            "feedback",
            "z-ci-helper",
            "--feedback",
            "accepted",
            "--agent",
            "codex",
            "--task",
            "write docs",
        ],
    );
    assert!(
        feedback_output.status.success(),
        "skill feedback should pass: {feedback}"
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(output.status.success(), "recommend should pass: {env}");
    assert_eq!(recommendation_results(&env)[0]["id"], json!("a-ci-helper"));
    let mismatched = recommendation(&env, "z-ci-helper");
    assert!(
        !mismatched["score_inputs"]
            .as_array()
            .expect("score inputs")
            .iter()
            .any(|input| input["field"] == json!("recommendation_feedback")),
        "feedback from another task must not affect ranking: {mismatched}"
    );
}

#[test]
fn recommend_exposes_error_rejected_and_ignored_telemetry_inputs() {
    let root = TestDir::new("recommend-telemetry-negative-evidence");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );
    let (error_output, error) = run_loom(
        root.path(),
        &[
            "skill",
            "used",
            "z-ci-helper",
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
    for feedback in ["rejected", "ignored"] {
        let (feedback_output, feedback_env) = run_loom(
            root.path(),
            &[
                "skill",
                "feedback",
                "z-ci-helper",
                "--feedback",
                feedback,
                "--agent",
                "codex",
                "--task",
                "fix failing ci",
            ],
        );
        assert!(
            feedback_output.status.success(),
            "skill feedback {feedback} should pass: {feedback_env}"
        );
    }

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(output.status.success(), "recommend should pass: {env}");
    assert_eq!(recommendation_results(&env)[0]["id"], json!("a-ci-helper"));
    let penalized = recommendation(&env, "z-ci-helper");
    let inputs = penalized["score_inputs"].as_array().expect("score inputs");
    assert!(
        inputs
            .iter()
            .any(|input| input["field"] == json!("telemetry_error_rate")
                && input["metric"] == json!("recent_error_rate")),
        "error telemetry should appear in score inputs: {penalized}"
    );
    assert!(
        inputs
            .iter()
            .any(|input| input["field"] == json!("recommendation_feedback")
                && input["metric"] == json!("rejected")),
        "rejected feedback should appear in score inputs: {penalized}"
    );
    assert!(
        inputs
            .iter()
            .any(|input| input["field"] == json!("recommendation_feedback")
                && input["metric"] == json!("ignored")
                && input["weight"] == json!(0)),
        "ignored feedback should be neutral evidence: {penalized}"
    );
}

#[test]
fn recommend_ignores_disabled_and_stale_telemetry() {
    let root = TestDir::new("recommend-disabled-stale-telemetry");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }

    let (baseline_output, baseline) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );
    assert!(
        baseline_output.status.success(),
        "baseline recommend should pass: {baseline}"
    );
    assert_eq!(
        recommendation_results(&baseline)[0]["id"],
        json!("a-ci-helper")
    );

    let (enable_output, enable) = run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        enable_output.status.success(),
        "enable should pass: {enable}"
    );
    let (disable_output, disable) = run_loom(root.path(), &["telemetry", "disable"]);
    assert!(
        disable_output.status.success(),
        "disable should pass: {disable}"
    );
    let (used_output, used) = run_loom(
        root.path(),
        &["skill", "used", "z-ci-helper", "--agent", "codex"],
    );
    assert!(
        used_output.status.success(),
        "disabled skill used should pass: {used}"
    );
    assert_eq!(used["data"]["recorded"], json!(false));
    assert!(
        !root.path().join("state/telemetry/events.jsonl").exists(),
        "disabled telemetry hook must not write events"
    );

    let (reenable_output, reenable) =
        run_loom(root.path(), &["telemetry", "enable", "--local-only"]);
    assert!(
        reenable_output.status.success(),
        "reenable should pass: {reenable}"
    );
    write_file(
        &root.path().join("state/telemetry/events.jsonl"),
        &format!(
            "{}\n{}\n",
            telemetry_line(
                "evt_stale_used",
                "skill.invocation",
                "z-ci-helper",
                "2000-01-01T00:00:00Z",
                json!({"success": true})
            ),
            telemetry_line(
                "evt_stale_feedback",
                "recommendation.feedback",
                "z-ci-helper",
                "2000-01-01T00:00:00Z",
                json!({"feedback": "accepted"})
            )
        ),
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(output.status.success(), "recommend should pass: {env}");
    assert_eq!(recommendation_results(&env)[0]["id"], json!("a-ci-helper"));
    let stale = recommendation(&env, "z-ci-helper");
    assert!(
        !stale["score_inputs"]
            .as_array()
            .expect("score inputs")
            .iter()
            .any(|input| input["field"].as_str().is_some_and(|field| field
                .starts_with("telemetry")
                || field == "recommendation_feedback")),
        "disabled or stale telemetry should not affect ranking: {stale}"
    );
}

#[test]
fn recommend_skips_malformed_optional_telemetry_config() {
    let root = TestDir::new("recommend-malformed-telemetry-config");
    for skill in ["a-ci-helper", "z-ci-helper"] {
        write_recommend_skill(
            root.path(),
            skill,
            "Use when fixing failing CI and test workflow failures.",
        );
    }
    write_file(
        &root.path().join("state/telemetry/config.json"),
        r#"{"schema_version":999,"enabled":true,"mode":"local-only","redaction":"default","retention_days":90}"#,
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "recommend", "fix failing ci", "--agent", "codex"],
    );

    assert!(
        output.status.success(),
        "malformed optional telemetry config should not block recommend: {env}"
    );
    assert_eq!(recommendation_results(&env)[0]["id"], json!("a-ci-helper"));
    let candidate = recommendation(&env, "a-ci-helper");
    assert!(
        candidate["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("telemetry evidence unavailable"))),
        "telemetry config problem should be downgraded to recommendation warning: {candidate}"
    );
}

#[test]
fn index_capabilities_keep_negative_fixtures_out_of_triggers() {
    let root = TestDir::new("recommend-positive-trigger-index");
    write_recommend_skill(
        root.path(),
        "trigger-helper",
        "Use when fixing failing CI and test workflow failures.",
    );
    write_file(
        &root
            .path()
            .join("skills/trigger-helper/evals/triggers.jsonl"),
        r#"{"id":"positive","prompt":"fix failing ci","expected_trigger":true}
{"id":"negative","prompt":"write product copy","expected_trigger":false}
"#,
    );

    let (output, env) = run_loom(root.path(), &["index", "build", "--no-embeddings"]);
    assert!(output.status.success(), "index build should pass: {env}");
    let raw = fs::read_to_string(root.path().join("state/index/skills.capabilities.json"))
        .expect("read capabilities index");
    let index: Value = serde_json::from_str(&raw).expect("parse capabilities index");
    let record = index["records"]
        .as_array()
        .expect("records")
        .iter()
        .find(|record| record["skill_id"] == json!("trigger-helper"))
        .expect("trigger-helper record");

    assert_eq!(record["triggers"], json!(["fix failing ci"]));
}

#[test]
fn index_does_not_mark_undeclared_dependencies_ready() {
    let root = TestDir::new("recommend-undeclared-deps-index");
    write_recommend_skill(
        root.path(),
        "plain-helper",
        "Use when fixing failing CI and test workflow failures.",
    );

    let (output, env) = run_loom(root.path(), &["index", "build", "--no-embeddings"]);
    assert!(output.status.success(), "index build should pass: {env}");
    let raw = fs::read_to_string(root.path().join("state/index/skills.capabilities.json"))
        .expect("read capabilities index");
    let index: Value = serde_json::from_str(&raw).expect("parse capabilities index");
    let record = index["records"]
        .as_array()
        .expect("records")
        .iter()
        .find(|record| record["skill_id"] == json!("plain-helper"))
        .expect("plain-helper record");

    assert_eq!(record["dependency_status"], json!("unknown"));
}

#[test]
fn resolve_selected_uses_evidence_adjusted_ranking() {
    let root = TestDir::new("recommend-resolve-evidence-selected");
    for skill in ["a-deploy-risk", "z-deploy-safe"] {
        write_recommend_skill(root.path(), skill, "Use when deploying release workflows.");
    }
    write_file(
        &root.path().join("skills/a-deploy-risk/loom.skill.toml"),
        r#"requires_tools = ["loom_missing_tool_378_definitely_absent"]
"#,
    );

    let (output, env) = run_loom(root.path(), &["skill", "resolve", "deploy release"]);

    assert!(output.status.success(), "resolve should pass: {env}");
    assert_eq!(
        env["data"]["selected"]["skill"]["skill_id"],
        json!("z-deploy-safe"),
        "resolve selected should follow evidence-adjusted ranking: {env}"
    );
}

#[test]
fn dependency_blocked_skillsets_are_penalized_in_ranking() {
    let root = TestDir::new("recommend-skillset-dependency-score");
    write_recommend_skill(
        root.path(),
        "blocked-member",
        "Use when deploying release workflows.",
    );
    write_recommend_skill(
        root.path(),
        "ready-member",
        "Use when deploying release workflows.",
    );
    write_file(
        &root.path().join("skills/blocked-member/loom.skill.toml"),
        r#"requires_tools = ["loom_missing_tool_378_definitely_absent"]
"#,
    );
    for (skillset, member) in [
        ("a-deploy-pack", "blocked-member"),
        ("z-deploy-pack", "ready-member"),
    ] {
        let (output, env) = run_loom(
            root.path(),
            &[
                "skillset",
                "create",
                skillset,
                "--description",
                "Deploy release workflow",
            ],
        );
        assert!(
            output.status.success(),
            "skillset create should pass: {env}"
        );
        let (output, env) = run_loom(root.path(), &["skillset", "add", skillset, member]);
        assert!(output.status.success(), "skillset add should pass: {env}");
    }

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "search",
            "deploy release",
            "--explain",
            "--agent",
            "codex",
        ],
    );

    assert!(output.status.success(), "search explain should pass: {env}");
    let blocked = recommendation(&env, "a-deploy-pack");
    let ready = recommendation(&env, "z-deploy-pack");
    assert_eq!(blocked["recommended_action"], json!("inspect"));
    assert!(
        blocked["score"].as_i64().unwrap() < ready["score"].as_i64().unwrap(),
        "dependency-blocked skillset should rank below ready equivalent: {env}"
    );
}
