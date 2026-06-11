use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};

fn git_stdout(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("user.name=Loom Test")
        .arg("-c")
        .arg("user.email=loom@example.invalid")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stderr={} stdout={}",
        args,
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn read_optional(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn seed_unregistered_skill_commit(root: &Path, skill: &str, body: &str, message: &str) {
    if !root.join(".git").exists() {
        git_stdout(root, &["init"]);
    }
    write_skill(root, skill, body);
    let skill_rel = format!("skills/{skill}");
    git_stdout(root, &["add", "--", &skill_rel]);
    git_stdout(root, &["commit", "-m", message, "--", &skill_rel]);
    assert!(
        !root.join("state/registry").exists(),
        "test helper should not initialize registry state"
    );
}

#[test]
fn rollback_dry_run_reports_preview_without_mutation() {
    let root = TestDir::new("rollback-dry-run-spelling");
    seed_unregistered_skill_commit(root.path(), "demo", "# Demo\n\nv1\n", "demo v1");
    write_skill(root.path(), "demo", "# Demo\n\nv2\n");
    git_stdout(root.path(), &["add", "skills/demo"]);
    git_stdout(
        root.path(),
        &["commit", "-m", "demo v2", "--", "skills/demo"],
    );
    let head_before = git_stdout(root.path(), &["rev-parse", "HEAD"]);

    let (output, env) = run_loom(
        root.path(),
        &["skill", "rollback", "demo", "--steps", "1", "--dry-run"],
    );

    assert!(
        output.status.success(),
        "dry-run failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["preview"], Value::Bool(true));
    assert_eq!(git_stdout(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert!(env["meta"]["op_id"].is_null());
}

#[test]
fn rollback_preview_reports_diff_and_projection_impact_without_mutation() {
    let root = TestDir::new("rollback-preview-impact");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    let target_path = root.path().join("live/claude-project-a");
    assert!(
        target_add(root.path(), "claude", &target_path, "managed")
            .0
            .status
            .success()
    );
    assert!(
        binding_add(
            root.path(),
            "claude",
            "default",
            "path-prefix",
            "/tmp/project-a",
            "target_claude_claude_project_a",
        )
        .0
        .status
        .success()
    );
    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        Some("copy"),
    );
    assert!(project_output.status.success(), "project should succeed");
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("projection instance id");

    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\nextra line\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    let head_before = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    let target_commit = git_stdout(root.path(), &["rev-parse", "HEAD~1"]);
    let status_before = git_stdout(root.path(), &["status", "--short"]);
    let tags_before = git_stdout(
        root.path(),
        &["tag", "--list", "recovery/model-onboarding/*"],
    );
    let operations_before = read_optional(&root.path().join("state/registry/ops/operations.jsonl"));
    let events_before = read_optional(&root.path().join("state/events/commands.jsonl"));

    let (preview_output, preview_env) = run_loom(
        root.path(),
        &[
            "skill",
            "rollback",
            "model-onboarding",
            "--steps",
            "1",
            "--preview",
        ],
    );
    assert!(
        preview_output.status.success(),
        "preview failed: stderr={} stdout={}",
        String::from_utf8_lossy(&preview_output.stderr),
        String::from_utf8_lossy(&preview_output.stdout)
    );
    assert_eq!(preview_env["ok"], Value::Bool(true));
    assert_eq!(preview_env["data"]["preview"], Value::Bool(true));
    assert_eq!(
        preview_env["data"]["reference"],
        Value::String("HEAD~1".to_string())
    );
    assert_eq!(
        preview_env["data"]["current_commit"],
        Value::String(head_before.clone())
    );
    assert_eq!(
        preview_env["data"]["target_commit"],
        Value::String(target_commit)
    );
    assert_eq!(preview_env["data"]["would_change"], Value::Bool(true));
    assert_eq!(preview_env["data"]["diff"]["files_changed"], Value::from(1));
    assert_eq!(
        preview_env["data"]["diff"]["changed_paths"][0],
        Value::String("skills/model-onboarding/SKILL.md".to_string())
    );
    assert_eq!(preview_env["data"]["diff"]["truncated"], Value::Bool(false));

    let projection = &preview_env["data"]["impacted_projections"][0];
    assert_eq!(
        projection["instance_id"],
        Value::String(instance_id.to_string())
    );
    assert_eq!(projection["method"], Value::String("copy".to_string()));
    assert_eq!(
        projection["live_path"],
        Value::String(
            target_path
                .join("model-onboarding")
                .canonicalize()
                .expect("canonical projected path")
                .to_string_lossy()
                .into_owned()
        )
    );
    assert_eq!(projection["requires_reproject"], Value::Bool(true));
    assert_eq!(
        preview_env["data"]["would_create_recovery_ref"],
        Value::Bool(true)
    );
    assert!(preview_env["meta"]["op_id"].is_null());

    assert_eq!(git_stdout(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_stdout(root.path(), &["status", "--short"]),
        status_before
    );
    assert_eq!(
        git_stdout(
            root.path(),
            &["tag", "--list", "recovery/model-onboarding/*"]
        ),
        tags_before
    );
    assert_eq!(
        read_optional(&root.path().join("state/registry/ops/operations.jsonl")),
        operations_before
    );
    assert_eq!(
        read_optional(&root.path().join("state/events/commands.jsonl")),
        events_before
    );
}

#[test]
fn rollback_preview_reports_noop_and_missing_registry_warning_without_state_write() {
    let root = TestDir::new("rollback-preview-no-registry");
    seed_unregistered_skill_commit(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
        "seed skill",
    );
    seed_unregistered_skill_commit(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
        "update skill",
    );

    let head_before = git_stdout(root.path(), &["rev-parse", "HEAD"]);
    let status_before = git_stdout(root.path(), &["status", "--short"]);
    let (preview_output, preview_env) = run_loom(
        root.path(),
        &[
            "skill",
            "rollback",
            "model-onboarding",
            "--to",
            "HEAD",
            "--preview",
        ],
    );
    assert!(
        preview_output.status.success(),
        "preview failed: stderr={} stdout={}",
        String::from_utf8_lossy(&preview_output.stderr),
        String::from_utf8_lossy(&preview_output.stdout)
    );
    assert_eq!(preview_env["ok"], Value::Bool(true));
    assert_eq!(preview_env["data"]["would_change"], Value::Bool(false));
    assert_eq!(
        preview_env["data"]["would_create_recovery_ref"],
        Value::Bool(false)
    );
    assert_eq!(preview_env["data"]["diff"]["files_changed"], Value::from(0));
    assert_eq!(
        preview_env["data"]["impacted_projections"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert!(
        preview_env["meta"]["warnings"][0]
            .as_str()
            .expect("registry warning")
            .contains("registry state not initialized"),
        "expected missing registry warning: {preview_env}"
    );
    assert_eq!(git_stdout(root.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git_stdout(root.path(), &["status", "--short"]),
        status_before
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "preview must not initialize registry state"
    );
    assert!(
        !root.path().join("state/events").exists(),
        "preview must not append command audit events"
    );
}
