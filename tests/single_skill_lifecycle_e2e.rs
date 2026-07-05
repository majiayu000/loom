mod common;

use std::fs;
use std::path::Path;

use common::{TestDir, run_loom, run_loom_with_env, write_file};
use serde_json::{Value, json};

fn assert_ok(output: &std::process::Output, env: &Value, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stdout={} stderr={} env={env}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["ok"], json!(true), "{context} should return ok=true");
}

fn run_with_home(root: &Path, home: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let home_arg = home.to_string_lossy().to_string();
    run_loom_with_env(root, &[("HOME", &home_arg)], args)
}

#[test]
fn single_skill_lifecycle_e2e_covers_create_validate_activate_release_rollback_deactivate() {
    let root = TestDir::new("single-skill-lifecycle-e2e");
    let home = TestDir::new("single-skill-lifecycle-e2e-home");
    let workspace = TestDir::new("single-skill-lifecycle-e2e-workspace");
    let skill = "lifecycle-demo";
    let workspace_arg = workspace.path().to_string_lossy().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "new",
            skill,
            "--template",
            "coding-workflow",
            "--description",
            "Use when verifying one complete Loom skill lifecycle.",
            "--agent",
            "codex",
        ],
    );
    assert_ok(&output, &env, "skill new");
    assert_eq!(env["data"]["created"], json!(true));

    for (context, args) in [
        ("skill lint", vec!["skill", "lint", skill, "--strict"]),
        ("skill inspect", vec!["skill", "inspect", skill]),
        ("skill scan", vec!["skill", "scan", skill]),
        (
            "skill deps",
            vec!["skill", "deps", skill, "--agent", "codex"],
        ),
        (
            "skill eval",
            vec!["skill", "eval", skill, "--agent", "codex"],
        ),
        (
            "skill improve",
            vec!["skill", "improve", skill, "--dry-run"],
        ),
    ] {
        let (output, env) = run_loom(root.path(), &args);
        assert_ok(&output, &env, context);
    }

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "commit",
            skill,
            "--from-source",
            "--message",
            "initial lifecycle skill",
        ],
    );
    assert_ok(&output, &env, "skill commit initial");

    let (output, env) = run_loom(root.path(), &["skill", "release", skill, "v1"]);
    assert_ok(&output, &env, "skill release");
    assert_eq!(env["data"]["tag"], json!("release/lifecycle-demo/v1"));

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            skill,
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert_ok(&output, &env, "skill activate");
    let projected = home.path().join(".agents/skills").join(skill);
    assert!(
        projected.exists(),
        "activation should materialize a projection"
    );

    for (context, args) in [
        (
            "skill visibility",
            vec![
                "skill",
                "visibility",
                skill,
                "--agent",
                "codex",
                "--workspace",
                &workspace_arg,
            ],
        ),
        (
            "skill diagnose",
            vec!["skill", "diagnose", skill, "--agent", "codex"],
        ),
    ] {
        let (output, env) = run_with_home(root.path(), home.path(), &args);
        assert_ok(&output, &env, context);
    }

    write_file(
        &root.path().join("skills").join(skill).join("SKILL.md"),
        "---\nname: lifecycle-demo\ndescription: Use when verifying one complete Loom skill lifecycle.\n---\n# lifecycle-demo\n\nUpdated lifecycle guidance.\n",
    );
    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "commit",
            skill,
            "--from-source",
            "--message",
            "update lifecycle skill",
        ],
    );
    assert_ok(&output, &env, "skill commit update");

    let (output, env) = run_loom(root.path(), &["skill", "rollback", skill, "--to", "HEAD~1"]);
    assert_ok(&output, &env, "skill rollback");
    let skill_body = fs::read_to_string(root.path().join("skills").join(skill).join("SKILL.md"))
        .expect("read rolled back skill");
    assert!(
        !skill_body.contains("Updated lifecycle guidance."),
        "rollback should restore the previous source revision"
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "deactivate",
            skill,
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert_ok(&output, &env, "skill deactivate");
    assert!(
        !projected.exists(),
        "deactivation should remove the symlink projection"
    );
}
