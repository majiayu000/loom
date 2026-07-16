mod common;

use std::fs;

use serde_json::{Value, json};

use common::{TestDir, run_loom, write_file};

fn read_file(root: &TestDir, rel: &str) -> String {
    fs::read_to_string(root.path().join(rel)).expect("read generated file")
}

#[test]
fn skill_new_creates_lint_clean_skill_skeleton() {
    let root = TestDir::new("skill-new-create");
    let description = "Use when diagnosing and fixing Rust test failures in a focused workflow.";

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "author",
            "new",
            "fixflow",
            "--template",
            "coding-workflow",
            "--description",
            description,
            "--agent",
            "codex",
        ],
    );

    assert!(
        output.status.success(),
        "skill author new failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["cmd"], json!("skill.author.new"));
    assert_eq!(env["data"]["skill"], json!("fixflow"));
    assert_eq!(env["data"]["template"], json!("coding-workflow"));
    assert_eq!(env["data"]["agent"], json!("codex"));
    assert_eq!(env["data"]["created"], Value::Bool(true));
    assert_eq!(env["data"]["lint"]["valid"], Value::Bool(true));
    assert_eq!(env["data"]["manifest"]["schema"], json!("loom.skill.v1"));
    assert_eq!(env["data"]["manifest"]["name"], json!("fixflow"));
    assert_eq!(
        env["data"]["files"].as_array().map(Vec::len),
        Some(7),
        "generated file list should stay stable"
    );

    let skill_md = read_file(&root, "skills/fixflow/SKILL.md");
    assert!(skill_md.contains("name: fixflow"));
    assert!(skill_md.contains(description));
    assert!(
        root.path()
            .join("skills/fixflow/evals/triggers.jsonl")
            .is_file()
    );
    assert!(
        root.path()
            .join("skills/fixflow/evals/tasks.jsonl")
            .is_file()
    );
    assert!(root.path().join("skills/fixflow/loom.skill.toml").is_file());

    let (lint_output, lint_env) = run_loom(root.path(), &["skill", "lint", "fixflow", "--strict"]);
    assert!(
        lint_output.status.success(),
        "generated skill should pass strict lint: stdout={} stderr={}",
        String::from_utf8_lossy(&lint_output.stdout),
        String::from_utf8_lossy(&lint_output.stderr)
    );
    assert_eq!(lint_env["data"]["valid"], Value::Bool(true));
}

#[test]
fn skill_new_dry_run_does_not_write_files_or_state() {
    let root = TestDir::new("skill-new-dry-run");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "author",
            "new",
            "draft-skill",
            "--template",
            "scripted",
            "--dry-run",
        ],
    );

    assert!(
        output.status.success(),
        "dry-run failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["data"]["created"], Value::Bool(false));
    assert_eq!(env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(env["data"]["previews"].as_array().map(Vec::len), Some(7));
    assert!(
        !root.path().join("skills/draft-skill").exists(),
        "dry-run must not create the skill directory"
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "dry-run must not initialize registry state"
    );
}

#[test]
fn skill_new_rejects_invalid_portable_name_without_source_writes() {
    let root = TestDir::new("skill-new-invalid");

    let (output, env) = run_loom(root.path(), &["skill", "author", "new", "Bad_Name"]);

    assert!(!output.status.success(), "invalid name should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(
        !root.path().join("skills").exists(),
        "invalid name should fail before writing source skill files"
    );
}

#[test]
fn skill_new_rejects_existing_skill_without_overwrite() {
    let root = TestDir::new("skill-new-existing");
    write_file(
        &root.path().join("skills/existing/SKILL.md"),
        "---\nname: existing\ndescription: Use when preserving an existing skill during create rejection.\n---\n# Existing\n",
    );
    let before = read_file(&root, "skills/existing/SKILL.md");

    let (output, env) = run_loom(root.path(), &["skill", "author", "new", "existing"]);

    assert!(!output.status.success(), "existing skill should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert_eq!(read_file(&root, "skills/existing/SKILL.md"), before);
    assert!(
        !root.path().join("skills/existing/loom.skill.toml").exists(),
        "failed create must not leave partial generated files"
    );
}
