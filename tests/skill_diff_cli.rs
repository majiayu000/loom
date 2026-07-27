mod common;

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use common::actions::save_skill;
use common::{TestDir, run_loom, write_skill};

fn assert_success(output: &std::process::Output, env: &Value, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: {env} stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn save_two_revisions(root: &Path) {
    write_skill(root, "demo", "# Demo\n\nv1\n");
    let (output, env) = save_skill(root, "demo");
    assert_success(&output, &env, "save v1");
    write_skill(root, "demo", "# Demo\n\nv2\n");
    let (output, env) = save_skill(root, "demo");
    assert_success(&output, &env, "save v2");
}

fn create_named_git_ref(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn skill_diff_accepts_git_valid_ref_characters() {
    let root = TestDir::new("skill-diff-valid-refs");
    save_two_revisions(root.path());
    create_named_git_ref(root.path(), &["branch", "feature@v1", "HEAD~1"]);
    create_named_git_ref(root.path(), &["tag", "release+candidate", "HEAD"]);
    create_named_git_ref(root.path(), &["branch", "发布/v1", "HEAD~1"]);

    for from in ["feature@v1", "发布/v1"] {
        let (output, env) = run_loom(
            root.path(),
            &["skill", "diff", "demo", from, "release+candidate"],
        );
        assert_success(&output, &env, from);
        assert_eq!(env["ok"], json!(true));
    }

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "diff",
            "demo",
            "--security",
            "feature@v1",
            "release+candidate",
        ],
    );
    assert_success(&output, &env, "security diff with Git-valid refs");
    assert_eq!(env["ok"], json!(true));
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

// Regression guard: `skill diff` used to forward `from`/`to` straight to
// `git diff`, so an option-shaped revision such as `--output=<path>` was
// executed by Git and wrote an arbitrary file. Revision arguments are now
// validated by `is_safe_git_ref` in src/validation.rs.
#[test]
fn skill_diff_rejects_option_shaped_revision_arguments() {
    let root = TestDir::new("skill-diff-unsafe-rev");
    save_two_revisions(root.path());

    let injected = root.path().join("injected.txt");
    let injected_arg = format!("--output={}", injected.display());

    let (output, env) = run_loom(
        root.path(),
        &["skill", "diff", "demo", "--", &injected_arg, "HEAD"],
    );

    assert!(
        !output.status.success(),
        "option-shaped revision must be rejected: {env}"
    );
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("must be a safe Git revision")),
        "unexpected error message: {}",
        env["error"]["message"]
    );
    assert!(
        !injected.exists(),
        "revision argument must never reach git as an option: git wrote {}",
        injected.display()
    );
}

#[test]
fn skill_diff_security_rejects_option_shaped_revision_arguments() {
    let root = TestDir::new("skill-diff-sec-inject");
    save_two_revisions(root.path());

    let injected = root.path().join("injected-security.txt");
    let injected_arg = format!("--output={}", injected.display());

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "diff",
            "demo",
            "--security",
            "--",
            &injected_arg,
            "HEAD",
        ],
    );

    assert!(
        !output.status.success(),
        "option-shaped revision unexpectedly accepted by --security: {env}"
    );
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(
        !injected.exists(),
        "revision argument must never reach git as an option: git wrote {}",
        injected.display()
    );
}

#[test]
fn skill_diff_rejects_revision_range_arguments() {
    let root = TestDir::new("skill-diff-range");
    save_two_revisions(root.path());

    let (output, env) = run_loom(
        root.path(),
        &["skill", "diff", "demo", "HEAD~1..HEAD", "HEAD"],
    );

    assert!(
        !output.status.success(),
        "range revision unexpectedly accepted"
    );
    assert_eq!(env["ok"], json!(false));
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
}
