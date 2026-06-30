mod common;

use std::fs;

use serde_json::{Value, json};

use common::{TestDir, run_loom, write_file, write_skill};

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
