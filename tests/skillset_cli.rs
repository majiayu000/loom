mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};

fn write_fixture_skill(root: &TestDir, skill: &str, description: &str) {
    write_skill(
        root.path(),
        skill,
        &format!("---\nname: {skill}\ndescription: {description}\n---\n# {skill}\n"),
    );
}

fn read_skillsets(root: &TestDir) -> Value {
    let raw = fs::read_to_string(root.path().join("state/registry/skillsets.json"))
        .expect("read skillsets");
    serde_json::from_str(&raw).expect("parse skillsets")
}

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn install_failing_pre_commit_hook(root: &Path) {
    let hook = root.join(".git/hooks/pre-commit");
    write_file(
        &hook,
        "#!/bin/sh\necho skillset rollback blocked >&2\nexit 1\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&hook).expect("hook metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook, perms).expect("make hook executable");
    }
}

#[test]
fn skillset_create_show_and_lint_empty_set() {
    let root = TestDir::new("skillset-create");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "create",
            "coding-flow",
            "--description",
            "Skills for coding tasks.",
        ],
    );
    assert!(output.status.success(), "create should pass: {env}");
    assert_eq!(env["cmd"], json!("skillset.create"));
    assert_eq!(env["data"]["skillset"]["id"], json!("coding-flow"));
    assert_eq!(
        env["data"]["skillset"]["description"],
        json!("Skills for coding tasks.")
    );
    assert_eq!(env["data"]["skillset"]["summary"]["members"], json!(0));
    assert!(root.path().join("state/registry/skillsets.json").is_file());

    let (output, env) = run_loom(root.path(), &["skillset", "show", "coding-flow"]);
    assert!(output.status.success(), "show should pass: {env}");
    assert_eq!(env["cmd"], json!("skillset.show"));
    assert_eq!(env["data"]["skillset"]["members"], json!([]));

    let (output, env) = run_loom(root.path(), &["skillset", "lint", "coding-flow"]);
    assert!(output.status.success(), "lint should pass: {env}");
    assert_eq!(env["data"]["valid"], json!(true));
    assert_eq!(env["data"]["summary"]["members"], json!(0));
    assert_eq!(env["data"]["findings"][0]["id"], json!("skillset_empty"));
    assert_eq!(env["data"]["findings"][0]["severity"], json!("warning"));
}

#[test]
fn skillset_create_rejects_duplicate_without_overwriting() {
    let root = TestDir::new("skillset-duplicate-create");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "initial create should pass: {env}");
    let before = read_skillsets(&root);

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(!output.status.success(), "duplicate create should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(read_skillsets(&root), before);
}

#[test]
fn skillset_add_show_and_remove_member() {
    let root = TestDir::new("skillset-add-remove");
    write_fixture_skill(
        &root,
        "fixflow",
        "Use when diagnosing and fixing failing tests.",
    );

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "add",
            "coding-flow",
            "fixflow",
            "--role",
            "execution",
        ],
    );
    assert!(output.status.success(), "add should pass: {env}");
    assert_eq!(env["cmd"], json!("skillset.add"));
    let member = &env["data"]["skillset"]["members"][0];
    assert_eq!(member["skill_id"], json!("fixflow"));
    assert_eq!(member["role"], json!("execution"));
    assert_eq!(member["required"], json!(true));
    assert_eq!(member["missing"], json!(false));
    assert_eq!(member["skill"]["skill_id"], json!("fixflow"));

    let (output, env) = run_loom(root.path(), &["skillset", "show", "coding-flow"]);
    assert!(output.status.success(), "show should pass: {env}");
    assert_eq!(env["data"]["skillset"]["summary"]["required"], json!(1));
    assert!(
        env["data"]["skillset"]["members"][0]["skill"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("failing tests")),
        "show should include skill read-model summary: {env}"
    );

    let source_before =
        fs::read_to_string(root.path().join("skills/fixflow/SKILL.md")).expect("read skill source");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "remove", "coding-flow", "fixflow"],
    );
    assert!(output.status.success(), "remove should pass: {env}");
    assert_eq!(env["data"]["skillset"]["summary"]["members"], json!(0));
    let source_after = fs::read_to_string(root.path().join("skills/fixflow/SKILL.md"))
        .expect("read skill source after remove");
    assert_eq!(source_after, source_before);
}

#[test]
fn skillset_add_rejects_missing_and_duplicate_member() {
    let root = TestDir::new("skillset-add-errors");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "coding-flow", "missing-skill"],
    );
    assert!(!output.status.success(), "missing skill should fail");
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));

    let (output, env) = run_loom(root.path(), &["skillset", "add", "coding-flow", "fixflow"]);
    assert!(output.status.success(), "first add should pass: {env}");

    let (output, env) = run_loom(root.path(), &["skillset", "add", "coding-flow", "fixflow"]);
    assert!(!output.status.success(), "duplicate member should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
}

#[test]
fn skillset_remove_rejects_missing_member() {
    let root = TestDir::new("skillset-remove-missing");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &["skillset", "remove", "coding-flow", "fixflow"],
    );
    assert!(
        !output.status.success(),
        "remove missing member should fail"
    );
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));
}

#[test]
fn skillset_lint_detects_manual_missing_required_member_drift() {
    let root = TestDir::new("skillset-lint-drift");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");
    let path = root.path().join("state/registry/skillsets.json");
    write_file(
        &path,
        r#"{
  "schema_version": 1,
  "skillsets": [
    {
      "id": "coding-flow",
      "description": null,
      "members": [
        {
          "skill_id": "missing-skill",
          "role": "execution",
          "required": true
        }
      ],
      "created_at": "2026-06-30T00:00:00Z",
      "updated_at": "2026-06-30T00:00:00Z"
    }
  ]
}
"#,
    );

    let (output, env) = run_loom(root.path(), &["skillset", "show", "coding-flow"]);
    assert!(
        output.status.success(),
        "show with drift should pass: {env}"
    );
    assert_eq!(
        env["data"]["skillset"]["members"][0]["missing"],
        json!(true)
    );
    assert_eq!(env["data"]["skillset"]["members"][0]["skill"], Value::Null);

    let (output, env) = run_loom(root.path(), &["skillset", "lint", "coding-flow"]);
    assert!(
        output.status.success(),
        "lint with drift should pass: {env}"
    );
    assert_eq!(env["data"]["valid"], json!(false));
    assert_eq!(env["data"]["summary"]["missing"], json!(1));
    assert_eq!(env["data"]["findings"][0]["id"], json!("member_missing"));
    assert_eq!(env["data"]["findings"][0]["severity"], json!("error"));
}

#[test]
fn skillset_activate_dry_run_and_apply_use_single_skill_activation() {
    let root = TestDir::new("skillset-activate");
    let home = TestDir::new("skillset-activate-home");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");
    write_fixture_skill(&root, "plan-flow", "Use when planning implementation work.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "coding-flow", "fixflow"]);
    assert!(output.status.success(), "add fixflow should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "coding-flow", "plan-flow"],
    );
    assert!(output.status.success(), "add plan-flow should pass: {env}");

    let home_str = home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &[
            "skillset",
            "activate",
            "coding-flow",
            "--agent",
            "codex",
            "--dry-run",
        ],
    );
    assert!(
        output.status.success(),
        "dry-run activate should pass: {env}"
    );
    assert_eq!(env["cmd"], json!("skillset.activate"));
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(env["data"]["summary"]["required_ready"], json!(2));
    assert_eq!(env["data"]["activation_plan"][0]["status"], json!("ready"));
    assert!(
        !home.path().join(".codex/skills/fixflow").exists(),
        "dry-run must not project skill"
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["skillset", "activate", "coding-flow", "--agent", "codex"],
    );
    assert!(output.status.success(), "activate should pass: {env}");
    assert_eq!(env["data"]["dry_run"], json!(false));
    assert_eq!(env["data"]["results"].as_array().map(Vec::len), Some(2));
    for result in env["data"]["results"]
        .as_array()
        .expect("activation results")
    {
        let materialized = result["result"]["projection"]["materialized_path"]
            .as_str()
            .expect("materialized path");
        assert!(
            fs::symlink_metadata(Path::new(materialized)).is_ok(),
            "activate should project member through the single-skill path: {materialized}"
        );
    }
}

#[test]
fn skillset_activate_rolls_back_partial_member_failure() {
    let root = TestDir::new("skillset-activate-partial");
    let home = TestDir::new("skillset-activate-partial-home");
    write_fixture_skill(&root, "alpha-flow", "Use when testing partial activation.");
    write_fixture_skill(&root, "beta-flow", "Use when testing partial activation.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "alpha-flow"]);
    assert!(output.status.success(), "add alpha should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "beta-flow"]);
    assert!(output.status.success(), "add beta should pass: {env}");

    let home_str = home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_str),
            ("LOOM_SKILLSET_ACTIVATE_FAULT_INJECT", "after:alpha-flow"),
        ],
        &["skillset", "activate", "bundle", "--agent", "codex"],
    );
    assert!(
        !output.status.success(),
        "fault-injected partial activation should fail"
    );
    assert_eq!(env["error"]["code"], json!("INTERNAL_ERROR"));
    assert_eq!(env["error"]["details"]["rollback_complete"], json!(true));
    assert_eq!(
        env["error"]["details"]["rollback"][0]["status"],
        json!("rolled_back")
    );
    let rolled_back_path = env["error"]["details"]["results_before_failure"][0]["result"]
        ["projection"]["materialized_path"]
        .as_str()
        .expect("rolled back materialized path");
    assert!(
        fs::symlink_metadata(Path::new(rolled_back_path)).is_err(),
        "partial activation should remove the already projected member"
    );
    assert!(
        fs::symlink_metadata(home.path().join(".agents/skills/beta-flow")).is_err(),
        "member after the fault should not be projected"
    );
}

#[test]
fn skillset_activate_rolls_back_current_member_after_inner_failure() {
    let root = TestDir::new("skillset-activate-current-failure");
    let home = TestDir::new("skillset-activate-current-failure-home");
    write_fixture_skill(
        &root,
        "alpha-flow",
        "Use when testing current member rollback.",
    );

    let (output, env) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "alpha-flow"]);
    assert!(output.status.success(), "add alpha should pass: {env}");

    let home_str = home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_str),
            (
                "LOOM_SKILL_ACTIVATE_FAULT_INJECT",
                "after_projection:alpha-flow",
            ),
        ],
        &["skillset", "activate", "bundle", "--agent", "codex"],
    );
    assert!(
        !output.status.success(),
        "fault-injected activation should fail: {env}"
    );
    assert_eq!(env["error"]["code"], json!("INTERNAL_ERROR"));
    assert_eq!(
        env["error"]["details"]["rollback"][0]["skill"],
        json!("alpha-flow")
    );
    assert!(
        fs::symlink_metadata(home.path().join(".agents/skills/alpha-flow")).is_err(),
        "rollback should remove the projection created by the failing member"
    );
}

#[test]
fn skillset_activate_preserves_preexisting_member_repair_on_later_failure() {
    let root = TestDir::new("skillset-activate-repair-preexisting");
    let home = TestDir::new("skillset-activate-repair-preexisting-home");
    write_fixture_skill(&root, "alpha-flow", "Use when testing repaired activation.");
    write_fixture_skill(&root, "beta-flow", "Use when testing repaired activation.");

    let home_str = home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["skill", "activate", "alpha-flow", "--agent", "codex"],
    );
    assert!(
        output.status.success(),
        "initial activate should pass: {env}"
    );

    let (output, env) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "alpha-flow"]);
    assert!(output.status.success(), "add alpha should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "beta-flow"]);
    assert!(output.status.success(), "add beta should pass: {env}");

    let alpha_projection = home.path().join(".agents/skills/alpha-flow");
    fs::remove_file(&alpha_projection).expect("remove alpha symlink to simulate drift");
    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_str),
            ("LOOM_SKILLSET_ACTIVATE_FAULT_INJECT", "after:beta-flow"),
        ],
        &["skillset", "activate", "bundle", "--agent", "codex"],
    );
    assert!(
        !output.status.success(),
        "fault-injected activation should fail: {env}"
    );
    assert!(
        fs::symlink_metadata(&alpha_projection).is_ok(),
        "rollback should not deactivate a pre-existing member repaired during skillset activation"
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["skill", "active", "list", "--agent", "codex"],
    );
    assert!(output.status.success(), "active list should pass: {env}");
    assert_eq!(env["data"]["count"], json!(1));
    assert_eq!(env["data"]["items"][0]["skill"], json!("alpha-flow"));
    assert_eq!(env["data"]["items"][0]["status"], json!("healthy"));
}

#[test]
fn skillset_activate_dry_run_ready_ignores_optional_missing_members() {
    let root = TestDir::new("skillset-activate-optional-dry-run");
    write_fixture_skill(
        &root,
        "required-flow",
        "Use when testing optional readiness.",
    );
    write_fixture_skill(
        &root,
        "optional-flow",
        "Use when testing optional readiness.",
    );

    let (output, env) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "required-flow"]);
    assert!(output.status.success(), "add required should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "bundle", "optional-flow", "--optional"],
    );
    assert!(output.status.success(), "add optional should pass: {env}");
    fs::remove_dir_all(root.path().join("skills/optional-flow")).expect("remove optional skill");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "activate",
            "bundle",
            "--agent",
            "codex",
            "--dry-run",
        ],
    );
    assert!(output.status.success(), "dry-run should pass: {env}");
    assert_eq!(env["data"]["ready"], json!(true));
    assert_eq!(env["data"]["summary"]["required_ready"], json!(1));
    assert_eq!(env["data"]["summary"]["optional_blocked"], json!(1));
}

#[test]
fn skillset_eval_aggregates_member_eval_results() {
    let root = TestDir::new("skillset-eval");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");
    write_fixture_skill(&root, "plan-flow", "Use when planning implementation work.");
    write_file(
        &root.path().join("skills/fixflow/evals/triggers.jsonl"),
        r#"{"id":"fix-trigger","prompt":"Use fixflow for this bug","expected_trigger":true,"observed_trigger":true}
"#,
    );
    write_file(
        &root.path().join("skills/plan-flow/evals/tasks.jsonl"),
        r#"{"id":"plan-task","task":"Plan the work","output":"Plan complete","trace":["read context"],"checks":{"outcome_contains":["Plan"],"process_contains":["context"]}}
"#,
    );

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "coding-flow", "fixflow"]);
    assert!(output.status.success(), "add fixflow should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "coding-flow", "plan-flow"],
    );
    assert!(output.status.success(), "add plan-flow should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "eval",
            "coding-flow",
            "--agent",
            "codex",
            "--baseline",
            "single-skills",
        ],
    );
    assert!(output.status.success(), "skillset eval should pass: {env}");
    assert_eq!(env["cmd"], json!("skillset.eval"));
    assert_eq!(env["data"]["baseline"], json!("single-skills"));
    assert_eq!(env["data"]["summary"]["case_count"], json!(2));
    assert_eq!(env["data"]["summary"]["passed"], json!(2));
    assert_eq!(env["data"]["summary"]["failed"], json!(0));
    assert_eq!(env["data"]["summary"]["aggregate_score"], json!(1.0));
    assert_eq!(env["data"]["members"].as_array().map(Vec::len), Some(2));
}

#[test]
fn skillset_eval_optional_failures_do_not_fail_bundle() {
    let root = TestDir::new("skillset-eval-optional-failure");
    write_fixture_skill(&root, "required-flow", "Use when testing required evals.");
    write_fixture_skill(&root, "optional-flow", "Use when testing optional evals.");
    write_file(
        &root.path().join("skills/required-flow/evals/tasks.jsonl"),
        r#"{"id":"required-task","task":"Run required eval","output":"Required done","trace":["read context"],"checks":{"outcome_contains":["Required"],"process_contains":["context"]}}
"#,
    );
    write_file(
        &root.path().join("skills/optional-flow/evals/tasks.jsonl"),
        r#"{"id":"optional-task","task":"Run optional eval","output":"Optional miss","trace":["read context"],"checks":{"outcome_contains":["Expected"],"process_contains":["context"]}}
"#,
    );

    let (output, env) = run_loom(root.path(), &["skillset", "create", "bundle"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "bundle", "required-flow"]);
    assert!(output.status.success(), "add required should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "add", "bundle", "optional-flow", "--optional"],
    );
    assert!(output.status.success(), "add optional should pass: {env}");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skillset",
            "eval",
            "bundle",
            "--agent",
            "codex",
            "--baseline",
            "single-skills",
        ],
    );
    assert!(
        output.status.success(),
        "optional eval failures should not fail the bundle: {env}"
    );
    assert_eq!(env["data"]["summary"]["failed"], json!(1));
    let optional = env["data"]["members"]
        .as_array()
        .expect("members")
        .iter()
        .find(|member| member["skill"] == json!("optional-flow"))
        .expect("optional member report");
    assert_eq!(optional["required"], json!(false));
    assert_eq!(optional["status"], json!("failed"));
}

#[test]
fn skillset_release_and_rollback_restore_definition() {
    let root = TestDir::new("skillset-release-rollback");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "release", "coding-flow", "v1.0.0"],
    );
    assert!(output.status.success(), "release should pass: {env}");
    assert_eq!(
        env["data"]["tag"],
        json!("release/skillset/coding-flow/v1.0.0")
    );

    let (output, env) = run_loom(root.path(), &["skillset", "add", "coding-flow", "fixflow"]);
    assert!(
        output.status.success(),
        "add after release should pass: {env}"
    );
    assert_eq!(env["data"]["skillset"]["summary"]["members"], json!(1));

    let (output, env) = run_loom(
        root.path(),
        &["skillset", "rollback", "coding-flow", "--to", "v1.0.0"],
    );
    assert!(output.status.success(), "rollback should pass: {env}");
    assert_eq!(env["cmd"], json!("skillset.rollback"));
    assert_eq!(
        env["data"]["reference"],
        json!("release/skillset/coding-flow/v1.0.0")
    );
    assert_eq!(
        env["data"]["skillset_record"]["summary"]["members"],
        json!(0)
    );

    let (output, env) = run_loom(root.path(), &["skillset", "show", "coding-flow"]);
    assert!(
        output.status.success(),
        "show after rollback should pass: {env}"
    );
    assert_eq!(env["data"]["skillset"]["summary"]["members"], json!(0));
}

#[test]
fn skillset_release_rejects_dirty_definition() {
    let root = TestDir::new("skillset-release-dirty-definition");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");

    let mut dirty = read_skillsets(&root);
    dirty["skillsets"][0]["description"] = json!("dirty release candidate");
    write_file(
        &root.path().join("state/registry/skillsets.json"),
        &serde_json::to_string_pretty(&dirty).expect("serialize dirty skillsets"),
    );

    let (output, env) = run_loom(
        root.path(),
        &["skillset", "release", "coding-flow", "v1.0.0"],
    );
    assert!(!output.status.success(), "dirty release should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        git_stdout(
            root.path(),
            &["tag", "--list", "release/skillset/coding-flow/v1.0.0"]
        ),
        ""
    );
}

#[test]
fn skillset_rollback_restores_index_after_commit_failure() {
    let root = TestDir::new("skillset-rollback-index-restore");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "release", "coding-flow", "v1.0.0"],
    );
    assert!(output.status.success(), "release should pass: {env}");
    let (output, env) = run_loom(root.path(), &["skillset", "add", "coding-flow", "fixflow"]);
    assert!(
        output.status.success(),
        "add after release should pass: {env}"
    );

    install_failing_pre_commit_hook(root.path());
    let before = read_skillsets(&root);
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "rollback", "coding-flow", "--to", "v1.0.0"],
    );
    assert!(
        !output.status.success(),
        "rollback commit should fail: {env}"
    );
    assert_eq!(read_skillsets(&root), before);
    assert_eq!(
        git_stdout(
            root.path(),
            &[
                "diff",
                "--cached",
                "--name-only",
                "--",
                "state/registry/skillsets.json",
            ],
        ),
        "",
        "failed rollback must not leave the rollback version staged"
    );
}

#[test]
fn skillset_rollback_rejects_missing_current_definition() {
    let root = TestDir::new("skillset-rollback-missing-current");
    write_fixture_skill(&root, "fixflow", "Use when fixing tests.");

    let (output, env) = run_loom(root.path(), &["skillset", "create", "coding-flow"]);
    assert!(output.status.success(), "create should pass: {env}");
    let (output, env) = run_loom(
        root.path(),
        &["skillset", "release", "coding-flow", "v1.0.0"],
    );
    assert!(output.status.success(), "release should pass: {env}");
    let mut current = read_skillsets(&root);
    current["skillsets"] = json!([]);
    write_file(
        &root.path().join("state/registry/skillsets.json"),
        &serde_json::to_string_pretty(&current).expect("serialize missing current skillsets"),
    );
    git_stdout(root.path(), &["add", "state/registry/skillsets.json"]);
    git_stdout(
        root.path(),
        &["commit", "-m", "skillset(coding-flow): remove definition"],
    );

    let (output, env) = run_loom(
        root.path(),
        &["skillset", "rollback", "coding-flow", "--to", "v1.0.0"],
    );
    assert!(
        !output.status.success(),
        "rollback must not recreate a missing current skillset"
    );
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));
    assert_eq!(read_skillsets(&root)["skillsets"], json!([]));
}
