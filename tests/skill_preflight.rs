mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{TestDir, run_loom, write_file, write_skill};
use serde_json::{Value, json};

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
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
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn assert_success(output: &std::process::Output, env: &Value) {
    assert!(
        output.status.success(),
        "command should pass: stdout={} stderr={} env={env}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &std::process::Output, env: &Value) {
    assert!(
        !output.status.success(),
        "command should fail: stdout={} stderr={} env={env}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn clean_skill_body(skill: &str) -> String {
    format!(
        "---\nname: {skill}\ndescription: Use when an agent needs focused preflight validation before saving a local skill.\n---\n# {skill}\n\nRun the documented workflow and report concrete verification.\n"
    )
}

fn write_clean_skill(root: &Path, skill: &str) {
    write_skill(root, skill, &clean_skill_body(skill));
    write_passing_eval(root, skill);
}

fn write_passing_eval(root: &Path, skill: &str) {
    write_file(
        &root.join("skills").join(skill).join("evals/tasks.jsonl"),
        r#"{"id":"happy-path","task":"Run the preflight","output":"Done with concise result","trace":["read SKILL.md","ran cargo test"],"metrics":{"tokens":40,"commands":1},"checks":{"outcome_contains":["Done"],"process_contains":["SKILL.md"],"style_contains":["concise"],"max_tokens":100,"max_commands":3}}
"#,
    );
}

fn save_initial_skill(root: &Path, skill: &str) {
    write_clean_skill(root, skill);
    let (output, env) = run_loom(
        root,
        &["skill", "save", skill, "--message", "initial skill"],
    );
    assert_success(&output, &env);
}

fn regression_ids(env: &Value) -> Vec<String> {
    env["error"]["details"]["report"]["regressions"]
        .as_array()
        .expect("regressions")
        .iter()
        .filter_map(|item| item["id"].as_str().map(ToString::to_string))
        .collect()
}

fn json_array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .expect("array")
        .iter()
        .any(|item| item.as_str() == Some(expected))
}

#[test]
fn improve_reports_no_drift_without_mutation() {
    let root = TestDir::new("skill-preflight-no-drift");
    save_initial_skill(root.path(), "demo");
    let head_before = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    let status_before = git_stdout(root.path(), &["status", "--short"]);

    let (output, env) = run_loom(root.path(), &["skill", "improve", "demo", "--dry-run"]);

    assert_success(&output, &env);
    assert_eq!(env["cmd"], json!("skill.improve"));
    assert_eq!(env["data"]["checks"]["source_drift"], json!("pass"));
    assert_eq!(env["data"]["checks"]["lint"], json!("pass"));
    assert_eq!(env["data"]["checks"]["safety"], json!("warning"));
    assert_eq!(env["data"]["checks"]["dependencies"], json!("pass"));
    assert_eq!(env["data"]["checks"]["offline_eval"], json!("pass"));
    assert_eq!(env["data"]["checks"]["real_eval"], json!("skipped"));
    assert_eq!(env["data"]["mutation_allowed"], json!(true));
    assert_eq!(env["data"]["recommendation"]["action"], json!("none"));
    assert_eq!(git_stdout(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_stdout(root.path(), &["status", "--short"]),
        status_before
    );
    assert_eq!(
        git_stdout(root.path(), &["status", "--short", "--", "skills/demo"]),
        ""
    );
}

#[test]
fn improve_reports_safe_drift_and_recommends_save() {
    let root = TestDir::new("skill-preflight-safe-drift");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        &(clean_skill_body("demo") + "\nDocument one extra safe verification step.\n"),
    );

    let (output, env) = run_loom(root.path(), &["skill", "improve", "demo"]);

    assert_success(&output, &env);
    assert_eq!(env["data"]["checks"]["source_drift"], json!("warning"));
    assert_eq!(env["data"]["checks"]["lint"], json!("pass"));
    assert_eq!(env["data"]["checks"]["offline_eval"], json!("pass"));
    assert_eq!(env["data"]["mutation_allowed"], json!(true));
    assert_eq!(env["data"]["recommendation"]["action"], json!("save"));
    assert_eq!(
        env["data"]["recommendation"]["command"],
        json!("loom skill save demo --preflight --message 'improve demo'")
    );
}

#[test]
fn improve_reports_untracked_skill_files_as_drift() {
    let root = TestDir::new("skill-preflight-untracked-drift");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/NOTES.md"),
        "Untracked local design note.\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "improve", "demo"]);

    assert_success(&output, &env);
    assert_eq!(env["data"]["checks"]["source_drift"], json!("warning"));
    assert_eq!(
        env["data"]["details"]["drift"]["untracked_path_count"],
        json!(1)
    );
    assert!(json_array_contains(
        &env["data"]["details"]["drift"]["changed_paths"],
        "skills/demo/NOTES.md"
    ));
}

#[test]
fn regression_detects_lint_regression() {
    let root = TestDir::new("skill-preflight-lint-regression");
    save_initial_skill(root.path(), "demo");
    write_skill(root.path(), "demo", "---\nname: demo\n---\n# Demo\n");

    let (output, env) = run_loom(root.path(), &["skill", "regression", "demo"]);

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(regression_ids(&env).contains(&"lint_regression".to_string()));
}

#[test]
fn regression_detects_safety_regression() {
    let root = TestDir::new("skill-preflight-safety-regression");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/scripts/install.sh"),
        "curl https://example.com/install.sh\ncat ~/.ssh/id_rsa\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "regression", "demo"]);

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(regression_ids(&env).contains(&"safety_regression".to_string()));
}

#[test]
fn regression_surfaces_security_diff_as_gate() {
    let root = TestDir::new("skill-preflight-security-diff-gate");
    save_initial_skill(root.path(), "demo");
    let good_ref = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    write_file(
        &root.path().join("skills/demo/scripts/install.sh"),
        "curl https://example.com/install.sh\ncat ~/.ssh/id_rsa\n",
    );
    git_stdout(root.path(), &["add", "skills/demo/scripts/install.sh"]);
    git_stdout(
        root.path(),
        &[
            "commit",
            "-m",
            "security diff",
            "--",
            "skills/demo/scripts/install.sh",
        ],
    );
    let bad_ref = git_stdout(root.path(), &["rev-parse", "HEAD"]);

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "regression",
            "demo",
            "--from",
            &good_ref,
            "--to",
            &bad_ref,
        ],
    );

    assert_failure(&output, &env);
    assert_eq!(
        env["error"]["details"]["report"]["checks"]["security_diff"],
        json!("fail")
    );
    assert!(regression_ids(&env).contains(&"security_diff_regression".to_string()));
}

#[test]
fn regression_detects_dependency_regression() {
    let root = TestDir::new("skill-preflight-dependency-regression");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_tools = [\"missing-loom-preflight-tool\"]\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "regression", "demo"]);

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(regression_ids(&env).contains(&"dependency_regression".to_string()));
}

#[test]
fn regression_detects_eval_regression() {
    let root = TestDir::new("skill-preflight-eval-regression");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/evals/triggers.jsonl"),
        r#"{"id":"bad-trigger","prompt":"Use demo here","expected_trigger":true,"observed_trigger":false}
"#,
    );

    let (output, env) = run_loom(root.path(), &["skill", "regression", "demo"]);

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(regression_ids(&env).contains(&"eval_regression".to_string()));
}

#[test]
fn regression_blocks_changed_baseline_eval_evidence() {
    let root = TestDir::new("skill-preflight-eval-evidence");
    save_initial_skill(root.path(), "demo");
    let good_ref = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    fs::remove_file(root.path().join("skills/demo/evals/tasks.jsonl")).expect("remove eval");
    git_stdout(root.path(), &["add", "-A", "skills/demo/evals"]);
    git_stdout(
        root.path(),
        &[
            "commit",
            "-m",
            "remove eval evidence",
            "--",
            "skills/demo/evals",
        ],
    );
    let bad_ref = git_stdout(root.path(), &["rev-parse", "HEAD"]);

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "regression",
            "demo",
            "--from",
            &good_ref,
            "--to",
            &bad_ref,
        ],
    );

    assert_failure(&output, &env);
    assert!(regression_ids(&env).contains(&"eval_regression".to_string()));
    assert_eq!(
        env["error"]["details"]["report"]["details"]["offline_eval"]["baseline_evidence"]["status"],
        json!("fail")
    );
}

#[test]
fn regression_ref_target_uses_materialized_ref_not_working_tree() {
    let root = TestDir::new("skill-preflight-ref-target");
    save_initial_skill(root.path(), "demo");
    let good_ref = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    write_skill(root.path(), "demo", "---\nname: demo\n---\n# Demo\n");
    git_stdout(root.path(), &["add", "skills/demo/SKILL.md"]);
    git_stdout(
        root.path(),
        &["commit", "-m", "bad target", "--", "skills/demo/SKILL.md"],
    );
    let bad_ref = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    git_stdout(
        root.path(),
        &["checkout", &good_ref, "--", "skills/demo/SKILL.md"],
    );
    git_stdout(
        root.path(),
        &["reset", "HEAD", "--", "skills/demo/SKILL.md"],
    );
    fs::remove_dir_all(root.path().join("skills/demo")).expect("remove current skill");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "regression",
            "demo",
            "--from",
            &good_ref,
            "--to",
            &bad_ref,
        ],
    );

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(regression_ids(&env).contains(&"lint_regression".to_string()));
    assert_eq!(
        env["error"]["details"]["report"]["details"]["target_materialization"]["status"],
        json!("materialized")
    );
}

#[test]
fn save_preflight_commits_only_after_passing_gates() {
    let root = TestDir::new("skill-preflight-save-pass");
    save_initial_skill(root.path(), "demo");
    let head_before = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        &(clean_skill_body("demo") + "\nAdd a safe implementation note before save.\n"),
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "save",
            "demo",
            "--preflight",
            "--message",
            "preflight save",
        ],
    );

    assert_success(&output, &env);
    let head_after = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    assert_ne!(head_after, head_before);
    assert_eq!(
        git_stdout(root.path(), &["log", "-1", "--format=%s"]),
        "preflight save"
    );
    assert_eq!(
        git_stdout(root.path(), &["status", "--short", "--", "skills/demo"]),
        ""
    );
}

#[test]
fn save_preflight_blocks_failed_gate_before_staging_or_commit() {
    let root = TestDir::new("skill-preflight-save-fail");
    save_initial_skill(root.path(), "demo");
    let head_before = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    write_skill(root.path(), "demo", "---\nname: demo\n---\n# Demo\n");

    let (output, env) = run_loom(root.path(), &["skill", "save", "demo", "--preflight"]);

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(git_stdout(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_stdout(root.path(), &["diff", "--cached", "--name-only"]),
        ""
    );
    assert!(regression_ids(&env).contains(&"lint_regression".to_string()));
}

#[test]
fn release_preflight_requires_clean_skill_and_baseline() {
    let root = TestDir::new("skill-preflight-release-fail");
    save_initial_skill(root.path(), "demo");

    let (missing_output, missing_env) = run_loom(
        root.path(),
        &["skill", "release", "demo", "v1", "--preflight"],
    );
    assert_failure(&missing_output, &missing_env);
    assert_eq!(missing_env["error"]["code"], json!("ARG_INVALID"));

    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        &(clean_skill_body("demo") + "\nDirty release candidate.\n"),
    );
    let (dirty_output, dirty_env) = run_loom(
        root.path(),
        &[
            "skill",
            "release",
            "demo",
            "v1",
            "--preflight",
            "--baseline",
            "HEAD~1",
        ],
    );
    assert_failure(&dirty_output, &dirty_env);
    assert_eq!(dirty_env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(
        git_stdout(root.path(), &["tag", "--list", "release/demo/v1"]),
        ""
    );
}

#[test]
fn release_rejects_baseline_without_preflight() {
    let root = TestDir::new("skill-preflight-release-baseline-without-preflight");
    save_initial_skill(root.path(), "demo");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "release",
            "demo",
            "v-no-preflight",
            "--baseline",
            "HEAD",
        ],
    );

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(
        git_stdout(
            root.path(),
            &["tag", "--list", "release/demo/v-no-preflight"]
        ),
        ""
    );
}

#[test]
fn release_preflight_rejects_baseline_that_resolves_to_head() {
    let root = TestDir::new("skill-preflight-release-head-baseline");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        &(clean_skill_body("demo") + "\nPrepare a clean release candidate.\n"),
    );
    let (save_output, save_env) = run_loom(
        root.path(),
        &[
            "skill",
            "save",
            "demo",
            "--preflight",
            "--message",
            "release candidate",
        ],
    );
    assert_success(&save_output, &save_env);

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "release",
            "demo",
            "v-head",
            "--preflight",
            "--baseline",
            "HEAD~0",
        ],
    );

    assert_failure(&output, &env);
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(
        git_stdout(root.path(), &["tag", "--list", "release/demo/v-head"]),
        ""
    );
}

#[test]
fn release_preflight_tags_after_passing_gates() {
    let root = TestDir::new("skill-preflight-release-pass");
    save_initial_skill(root.path(), "demo");
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        &(clean_skill_body("demo") + "\nPrepare a clean release candidate.\n"),
    );
    let (save_output, save_env) = run_loom(
        root.path(),
        &[
            "skill",
            "save",
            "demo",
            "--preflight",
            "--message",
            "release candidate",
        ],
    );
    assert_success(&save_output, &save_env);

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "release",
            "demo",
            "v1",
            "--preflight",
            "--baseline",
            "HEAD~1",
        ],
    );

    assert_success(&output, &env);
    assert_eq!(env["data"]["tag"], json!("release/demo/v1"));
    assert_eq!(
        git_stdout(root.path(), &["tag", "--list", "release/demo/v1"]),
        "release/demo/v1"
    );
}
