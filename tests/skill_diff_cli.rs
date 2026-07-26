use serde_json::Value;

mod common;

use common::actions::save_skill;
use common::{TestDir, run_loom, write_skill};

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn setup_skill_with_two_commits(root: &TestDir) {
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v1");
    write_skill(root.path(), "demo", "# Demo\n\nv2\n");
    assert_success(&save_skill(root.path(), "demo").0, "save v2");
}

#[test]
fn skill_diff_returns_patch_for_safe_revisions() {
    let root = TestDir::new("skill-diff-happy");
    setup_skill_with_two_commits(&root);

    let (output, env) = run_loom(root.path(), &["skill", "diff", "demo", "HEAD~1", "HEAD"]);

    assert_success(&output, "skill diff");
    assert_eq!(env["ok"], Value::Bool(true));
    let diff = env["data"]["diff"].as_str().expect("diff text");
    assert!(
        diff.contains("v2"),
        "diff should include new content: {diff}"
    );
}

#[test]
fn skill_diff_rejects_option_shaped_revision_arguments() {
    let root = TestDir::new("skill-diff-inject");
    setup_skill_with_two_commits(&root);

    let injected = root.path().join("injected.txt");
    let injected_arg = format!("--output={}", injected.display());

    let (output, env) = run_loom(
        root.path(),
        &["skill", "diff", "demo", "--", &injected_arg, "HEAD"],
    );

    assert!(
        !output.status.success(),
        "option-shaped revision unexpectedly accepted: stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("must be a safe Git revision")),
        "unexpected error message: {}",
        env["error"]["message"]
    );
    assert!(
        !injected.exists(),
        "git wrote {} — revision argument was passed through unvalidated",
        injected.display()
    );
}

#[test]
fn skill_diff_security_rejects_option_shaped_revision_arguments() {
    let root = TestDir::new("skill-diff-sec-inject");
    setup_skill_with_two_commits(&root);

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
        "option-shaped revision unexpectedly accepted by --security: stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert!(
        !injected.exists(),
        "git wrote {} — revision argument was passed through unvalidated",
        injected.display()
    );
}

#[test]
fn skill_diff_rejects_revision_range_arguments() {
    let root = TestDir::new("skill-diff-range");
    setup_skill_with_two_commits(&root);

    let (output, env) = run_loom(
        root.path(),
        &["skill", "diff", "demo", "HEAD~1..HEAD", "HEAD"],
    );

    assert!(
        !output.status.success(),
        "range revision unexpectedly accepted"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}
