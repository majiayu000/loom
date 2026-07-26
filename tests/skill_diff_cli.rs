mod common;

use serde_json::{Value, json};

use common::actions::save_skill;
use common::{TestDir, run_loom, write_skill};

fn assert_success(output: &std::process::Output, env: &Value, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: {env} stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn save_two_revisions(root: &std::path::Path) {
    write_skill(root, "demo", "# Demo\n\nv1\n");
    let (output, env) = save_skill(root, "demo");
    assert_success(&output, &env, "save v1");
    write_skill(root, "demo", "# Demo\n\nv2\n");
    let (output, env) = save_skill(root, "demo");
    assert_success(&output, &env, "save v2");
}

#[test]
fn skill_diff_reports_source_patch_between_revisions() {
    let root = TestDir::new("skill-diff");
    save_two_revisions(root.path());

    let (output, env) = run_loom(root.path(), &["skill", "diff", "demo", "HEAD~1", "HEAD"]);

    assert_success(&output, &env, "diff");
    assert_eq!(env["ok"], json!(true));
    assert_eq!(env["cmd"], json!("skill.diff"));
    assert_eq!(env["data"]["skill"], json!("demo"));
    assert_eq!(env["data"]["from"], json!("HEAD~1"));
    assert_eq!(env["data"]["to"], json!("HEAD"));
    let diff = env["data"]["diff"].as_str().expect("diff text");
    assert!(
        diff.contains("skills/demo/SKILL.md"),
        "diff should be scoped to the skill source: {diff}"
    );
    assert!(
        diff.contains("-v1"),
        "diff should show removed line: {diff}"
    );
    assert!(diff.contains("+v2"), "diff should show added line: {diff}");
}

#[test]
fn skill_diff_between_identical_revisions_is_empty() {
    let root = TestDir::new("skill-diff-noop");
    save_two_revisions(root.path());

    let (output, env) = run_loom(root.path(), &["skill", "diff", "demo", "HEAD", "HEAD"]);

    assert_success(&output, &env, "noop diff");
    assert_eq!(env["data"]["diff"], json!(""));
}

#[test]
fn skill_diff_unknown_skill_fails_without_side_effects() {
    let root = TestDir::new("skill-diff-missing");

    let (output, env) = run_loom(root.path(), &["skill", "diff", "demo", "HEAD~1", "HEAD"]);

    assert!(!output.status.success(), "diff unexpectedly succeeded");
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));
    assert!(!root.path().join(".git").exists());
    assert!(!root.path().join("state/registry").exists());
}

// BUG: `skill diff` forwards `from`/`to` to `git diff` without the
// safe-revision validation that `skill history` applies
// (`is_safe_history_ref` in src/commands/history_cmds.rs). An
// option-shaped revision such as `--output=<path>` is executed by Git and
// writes an arbitrary file. This test encodes the intended contract
// (ARG_INVALID, no side effects) and stays ignored until cmd_diff
// validates its revision arguments.
#[test]
#[ignore = "skill diff currently forwards option-shaped revisions to git (argument injection); see PR body"]
fn skill_diff_rejects_option_shaped_revision_arguments() {
    let root = TestDir::new("skill-diff-unsafe-rev");
    save_two_revisions(root.path());
    let injected = root.path().join("injected.txt");
    let output_arg = format!("--output={}", injected.display());

    let (output, env) = run_loom(
        root.path(),
        &["skill", "diff", "demo", "--", &output_arg, "HEAD"],
    );

    assert!(
        !output.status.success(),
        "option-shaped revision must be rejected: {env}"
    );
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(
        !injected.exists(),
        "revision argument must never reach git as an option"
    );
}

#[test]
fn skill_diff_unknown_revision_fails_with_git_error() {
    let root = TestDir::new("skill-diff-bad-rev");
    save_two_revisions(root.path());

    let (output, env) = run_loom(
        root.path(),
        &["skill", "diff", "demo", "no-such-revision", "HEAD"],
    );

    assert!(!output.status.success(), "diff unexpectedly succeeded");
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("GIT_ERROR"));
}
