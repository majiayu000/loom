mod common;

use std::fs;

use serde_json::{Value, json};

use common::{TestDir, run_loom, write_file, write_skill};

#[test]
fn use_plan_is_read_only_before_apply() {
    let root = TestDir::new("use-plan");
    let workspace = TestDir::new("use-plan-workspace");
    write_skill(
        root.path(),
        "pdf-helper",
        "---\nname: pdf-helper\ndescription: Use when working with PDF documents in an agent workflow.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude,codex",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
        ],
    );

    assert!(output.status.success(), "use plan should pass: {env}");
    assert_eq!(env["cmd"], json!("use"));
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(env["data"]["steps"].as_array().map(Vec::len), Some(2));
    assert!(
        env["data"]["next_actions"][0]
            .as_str()
            .is_some_and(|command| command.contains("--apply")),
        "plan should point at explicit apply command: {env}"
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "plan mode must not initialize registry state"
    );
    assert!(
        !root.path().join("targets").exists(),
        "plan mode must not create managed targets"
    );
}

#[test]
fn use_apply_projects_local_skill_without_manual_ids() {
    let root = TestDir::new("use-apply");
    let source = TestDir::new("use-apply-source");
    let workspace = TestDir::new("use-apply-workspace");
    write_file(
        &source.path().join("SKILL.md"),
        "---\nname: pdf-helper\ndescription: Use when testing the human friendly use flow end to end.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(root.path(), &["workspace", "init"]);
    assert!(output.status.success(), "workspace init should pass: {env}");

    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "pdf-helper"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");

    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (output, env) = run_loom(
        root.path(),
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude",
            "--scope",
            "project",
            "--workspace",
            workspace_arg,
            "--profile",
            "test",
            "--method",
            "copy",
            "--apply",
        ],
    );

    assert!(output.status.success(), "use apply should pass: {env}");
    assert_eq!(env["data"]["dry_run"], json!(false));
    let applied = &env["data"]["applied"][0];
    assert_eq!(applied["agent"], json!("claude"));
    assert!(applied["target"]["target_id"].as_str().is_some());
    assert!(applied["binding"]["binding_id"].as_str().is_some());
    assert!(applied["projection"]["instance_id"].as_str().is_some());
    assert!(applied["operation_ids"]["target"].as_str().is_some());
    assert!(applied["operation_ids"]["binding"].as_str().is_some());
    assert!(applied["operation_ids"]["projection"].as_str().is_some());

    let projection_path = applied["projection"]["materialized_path"]
        .as_str()
        .expect("projection path");
    assert!(
        fs::read_to_string(format!("{projection_path}/SKILL.md"))
            .expect("read projected skill")
            .contains("PDF helper"),
        "use apply should materialize the selected local skill"
    );

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(output.status.success(), "binding list should pass: {env}");
    assert_eq!(env["data"]["count"], Value::from(1));

    let (output, env) = run_loom(root.path(), &["target", "list"]);
    assert!(output.status.success(), "target list should pass: {env}");
    assert_eq!(env["data"]["count"], Value::from(1));
}
