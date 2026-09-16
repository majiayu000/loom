mod common;

use std::fs;

use serde_json::json;

use common::{TestDir, run_loom, write_file, write_skill};

fn bundle() -> TestDir {
    let root = TestDir::new("skillset-e2e");
    let (output, value) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(output.status.success(), "{value}");
    for skill in ["plan", "fix"] {
        write_skill(
            root.path(),
            skill,
            &format!(
                "---\nname: {skill}\ndescription: Use when testing {skill}.\n---\n# {skill}\n"
            ),
        );
        let (output, value) = run_loom(root.path(), &["skillset", "add", "bundle", skill]);
        assert!(output.status.success(), "{value}");
    }
    write_file(
        &root.path().join("skillsets/bundle/evals/tasks.jsonl"),
        "{\"id\":\"whole-task\",\"prompt\":\"Plan then fix the task\",\"checks\":{\"outcome_contains\":[\"task complete\"],\"exit_code\":0}}\n",
    );
    root
}

#[test]
fn compares_bundle_to_no_skill_and_each_single_skill_without_source_writes() {
    let root = bundle();
    let source = fs::read(root.path().join("skills/fix/SKILL.md")).unwrap();
    for baseline in ["no-skill", "single-skills"] {
        let (output, value) = run_loom(
            root.path(),
            &[
                "skillset",
                "eval",
                "bundle",
                "--agent",
                "codex",
                "--runner",
                "mock",
                "--baseline",
                baseline,
            ],
        );
        assert!(output.status.success(), "{value}");
        let report = &value["data"]["end_to_end"];
        assert_eq!(report["status"], "passed");
        assert_eq!(report["synthetic"], true);
        assert_eq!(report["summary"]["case_count"], 1);
        assert_eq!(report["summary"]["passed"], 1);
        let runs = report["runs"]["baselines"].as_array().unwrap();
        assert_eq!(runs.len(), if baseline == "no-skill" { 1 } else { 2 });
        if baseline == "no-skill" {
            assert_eq!(runs[0]["pass_rate"], 0.0);
        } else {
            assert_eq!(runs[0]["skill"], "fix");
            assert_eq!(runs[1]["skill"], "plan");
        }
        for run in report["runs"]["bundle"].as_array().unwrap() {
            assert!(!std::path::Path::new(run["workspace"].as_str().unwrap()).exists());
        }
    }
    assert_eq!(
        fs::read(root.path().join("skills/fix/SKILL.md")).unwrap(),
        source
    );
    assert!(!root.path().join("skills/bundle").exists());
}

#[test]
fn explicit_runner_preview_does_not_start_codex() {
    let root = bundle();
    let (output, value) = run_loom(
        root.path(),
        &[
            "skillset",
            "eval",
            "bundle",
            "--agent",
            "codex",
            "--runner",
            "codex-cli",
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "{value}");
    assert_eq!(value["data"]["end_to_end"]["status"], "planned");
    assert_eq!(
        value["data"]["end_to_end"]["members"],
        json!(["fix", "plan"])
    );
    assert!(value["data"]["end_to_end"]["runs"].is_null());
}

#[test]
fn bundle_failures_propagate_and_empty_or_malformed_cases_fail() {
    let root = bundle();
    let cases = root.path().join("skillsets/bundle/evals/tasks.jsonl");
    for (body, expected) in [
        (
            "{\"prompt\":\"fix\",\"checks\":{\"outcome_contains\":[\"unreachable expected result\"]}}\n",
            "EVAL_FAILED",
        ),
        ("malformed\n", "SCHEMA_MISMATCH"),
        ("", "ARG_INVALID"),
        ("{}\n", "ARG_INVALID"),
    ] {
        write_file(&cases, body);
        let (output, value) = run_loom(
            root.path(),
            &[
                "skillset", "eval", "bundle", "--agent", "codex", "--runner", "mock",
            ],
        );
        assert!(!output.status.success(), "{value}");
        assert_eq!(value["error"]["code"], expected, "{value}");
    }
}

#[test]
fn quarantined_member_blocks_bundle_runner() {
    let root = bundle();
    let (output, value) = run_loom(
        root.path(),
        &["skill", "quarantine", "fix", "--reason", "review needed"],
    );
    assert!(output.status.success(), "{value}");
    let (output, value) = run_loom(
        root.path(),
        &[
            "skillset", "eval", "bundle", "--agent", "codex", "--runner", "mock",
        ],
    );
    assert!(!output.status.success(), "{value}");
    assert_eq!(value["error"]["code"], "POLICY_BLOCKED");
}
