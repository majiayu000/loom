use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, run_loom_with_env, write_skill};

fn write_example_skill(root: &std::path::Path, skill: &str) {
    write_skill(root, skill, &format!("# {}\n\nexample skill\n", skill));
}

fn read_operations_log(root: &std::path::Path) -> String {
    fs::read_to_string(root.join("state/registry/ops/operations.jsonl"))
        .expect("read operations log")
}

fn read_checkpoint(root: &std::path::Path) -> String {
    fs::read_to_string(root.join("state/registry/ops/checkpoint.json")).expect("read checkpoint")
}

fn read_observation_log(root: &std::path::Path, instance_id: &str) -> String {
    fs::read_to_string(
        root.join("state/registry/observations")
            .join(format!("{instance_id}.jsonl")),
    )
    .expect("read observation log")
}

fn git_output(root: &Path, args: &[&str]) -> String {
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
        "git {:?} failed: stderr={} stdout={}",
        args,
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_head(root: &Path) -> String {
    git_output(root, &["rev-parse", "HEAD"])
}

fn git_status_short_for(root: &Path, pathspecs: &[&str]) -> String {
    let mut args = vec!["status", "--short", "--"];
    args.extend(pathspecs);
    git_output(root, &args)
}

fn git_tag_exists(root: &Path, tag: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(["rev-parse", "--verify", "--quiet", tag])
        .status()
        .expect("run git")
        .success()
}

#[test]
fn skill_project_creates_projection_rule_and_instance() {
    let root = TestDir::new("registry-skill-project");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_claude_project_a",
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        None,
    );
    assert!(
        project_output.status.success(),
        "project failed: stderr={} stdout={}",
        String::from_utf8_lossy(&project_output.stderr),
        String::from_utf8_lossy(&project_output.stdout)
    );
    assert_eq!(project_env["ok"], Value::Bool(true));
    assert_eq!(
        project_env["data"]["projection"]["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
    assert_eq!(
        project_env["data"]["projection"]["target_id"],
        Value::String("target_claude_claude_project_a".to_string())
    );
    assert_eq!(
        project_env["data"]["projection"]["method"],
        Value::String("symlink".to_string())
    );
    assert_eq!(
        project_env["meta"]["op_id"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("projection instance id");
    let observations = read_observation_log(root.path(), instance_id);
    assert!(observations.contains("\"kind\":\"projected\""));
    assert!(observations.contains(&format!("\"instance_id\":\"{instance_id}\"")));

    let projected_path = target_path.join("model-onboarding");
    assert!(projected_path.exists(), "projected path should exist");

    let (binding_show_output, binding_show_env) = run_loom(
        root.path(),
        &["workspace", "binding", "show", "bind_claude_project_a"],
    );
    assert!(
        binding_show_output.status.success(),
        "binding show should succeed"
    );
    assert_eq!(
        binding_show_env["data"]["rules"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        binding_show_env["data"]["projections"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn skill_rollback_records_operation_and_observation() {
    let root = TestDir::new("registry-skill-rollback-audit");
    write_example_skill(root.path(), "model-onboarding");

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
        .expect("projection instance id")
        .to_string();

    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    let (rollback_output, rollback_env) = run_loom(
        root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~1"],
    );
    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(rollback_env["ok"], Value::Bool(true));
    assert_eq!(
        rollback_env["meta"]["op_id"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );

    let operations = read_operations_log(root.path());
    assert!(operations.contains("\"intent\":\"skill.rollback\""));
    assert!(operations.contains("\"reference\":\"HEAD~1\""));

    let observations = read_observation_log(root.path(), &instance_id);
    assert!(observations.contains("\"kind\":\"rollback\""));

    let source = fs::read_to_string(root.path().join("skills/model-onboarding/SKILL.md"))
        .expect("read source skill");
    assert!(source.contains("example skill"));
    assert!(!source.contains("source v2"));
}

#[test]
fn skill_rollback_noop_does_not_initialize_registry() {
    let root = TestDir::new("registry-skill-rollback-noop");
    write_example_skill(root.path(), "model-onboarding");
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    let (rollback_output, rollback_env) = run_loom(
        root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD"],
    );
    assert!(
        rollback_output.status.success(),
        "rollback noop failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(rollback_env["ok"], Value::Bool(true));
    assert_eq!(rollback_env["data"]["noop"], Value::Bool(true));
    assert!(
        !root.path().join("state/registry").exists(),
        "noop rollback should not create registry state"
    );
    assert!(
        !root.path().join("state/backups").exists(),
        "noop rollback should not leave skill backups"
    );
}

#[test]
fn skill_rollback_rolls_back_commits_and_worktree_after_late_audit_failure() {
    let root = TestDir::new("registry-skill-rollback-late-audit-rollback");
    write_example_skill(root.path(), "model-onboarding");

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
        .expect("projection instance id")
        .to_string();

    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    let operations_before = read_operations_log(root.path());
    let checkpoint_before = read_checkpoint(root.path());
    let observations_before = read_observation_log(root.path(), &instance_id);
    let head_before = git_head(root.path());

    let (rollback_output, rollback_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_rollback_after_state_commit")],
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~1"],
    );

    assert!(
        !rollback_output.status.success(),
        "rollback unexpectedly succeeded"
    );
    assert_eq!(rollback_env["ok"], Value::Bool(false));
    assert_eq!(git_head(root.path()), head_before);
    assert_eq!(read_operations_log(root.path()), operations_before);
    assert_eq!(read_checkpoint(root.path()), checkpoint_before);
    assert_eq!(
        read_observation_log(root.path(), &instance_id),
        observations_before
    );
    let source = fs::read_to_string(root.path().join("skills/model-onboarding/SKILL.md"))
        .expect("read source skill");
    assert!(source.contains("source v2"));
    assert_eq!(
        git_status_short_for(root.path(), &["skills/model-onboarding", "state/registry"]),
        ""
    );
}

#[test]
fn skill_rollback_removes_new_registry_layout_after_late_audit_failure() {
    let root = TestDir::new("registry-skill-rollback-new-layout-rollback");
    write_example_skill(root.path(), "model-onboarding");
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
    );
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "precondition: registry should not exist before rollback"
    );
    let head_before = git_head(root.path());

    let (rollback_output, rollback_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_rollback_after_state_commit")],
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~1"],
    );

    assert!(
        !rollback_output.status.success(),
        "rollback unexpectedly succeeded"
    );
    assert_eq!(rollback_env["ok"], Value::Bool(false));
    assert_eq!(git_head(root.path()), head_before);
    assert!(
        !root.path().join("state/registry").exists(),
        "failed rollback should remove newly-created registry layout"
    );
    let source = fs::read_to_string(root.path().join("skills/model-onboarding/SKILL.md"))
        .expect("read source skill");
    assert!(source.contains("source v2"));
    assert_eq!(
        git_status_short_for(root.path(), &["skills/model-onboarding", "state/registry"]),
        ""
    );
}

#[test]
fn skill_release_records_operation() {
    let root = TestDir::new("registry-skill-release-audit");
    write_example_skill(root.path(), "model-onboarding");
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );

    let (release_output, release_env) = run_loom(
        root.path(),
        &["skill", "release", "model-onboarding", "v1.0.0"],
    );
    assert!(
        release_output.status.success(),
        "release failed: stderr={} stdout={}",
        String::from_utf8_lossy(&release_output.stderr),
        String::from_utf8_lossy(&release_output.stdout)
    );
    assert_eq!(release_env["ok"], Value::Bool(true));
    assert_eq!(
        release_env["meta"]["op_id"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );

    let operations = read_operations_log(root.path());
    assert!(operations.contains("\"intent\":\"skill.release\""));
    assert!(operations.contains("\"version\":\"v1.0.0\""));
}

#[test]
fn skill_release_removes_new_registry_layout_after_late_failure() {
    let root = TestDir::new("registry-skill-release-new-layout-rollback");
    write_example_skill(root.path(), "model-onboarding");
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "precondition: registry should not exist before release"
    );
    let head_before = git_head(root.path());

    let (release_output, release_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_release_after_state_commit")],
        &["skill", "release", "model-onboarding", "v1.0.0"],
    );

    assert!(
        !release_output.status.success(),
        "release unexpectedly succeeded"
    );
    assert_eq!(release_env["ok"], Value::Bool(false));
    assert_eq!(git_head(root.path()), head_before);
    assert!(
        !git_tag_exists(root.path(), "release/model-onboarding/v1.0.0"),
        "failed release should delete the release tag"
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "failed release should remove newly-created registry layout"
    );
    assert_eq!(git_status_short_for(root.path(), &["state/registry"]), "");
}

#[test]
fn skill_release_removes_new_registry_layout_when_tag_creation_fails() {
    let root = TestDir::new("registry-skill-release-tag-failure");
    write_example_skill(root.path(), "model-onboarding");
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success()
    );
    git_output(
        root.path(),
        &[
            "tag",
            "-a",
            "release/model-onboarding/v1.0.0",
            "-m",
            "existing release",
        ],
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "precondition: registry should not exist before release"
    );
    let head_before = git_head(root.path());

    let (release_output, release_env) = run_loom(
        root.path(),
        &["skill", "release", "model-onboarding", "v1.0.0"],
    );

    assert!(
        !release_output.status.success(),
        "release unexpectedly succeeded"
    );
    assert_eq!(release_env["ok"], Value::Bool(false));
    assert_eq!(git_head(root.path()), head_before);
    assert!(
        !root.path().join("state/registry").exists(),
        "failed release should remove newly-created registry layout"
    );
    assert!(
        !root.path().join("state/backups").exists(),
        "failed release should remove registry layout backups"
    );
}

#[test]
fn skill_release_rolls_back_audit_commit_and_tag_after_late_failure() {
    let root = TestDir::new("registry-skill-release-audit-rollback");
    write_example_skill(root.path(), "model-onboarding");
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

    let operations_before = read_operations_log(root.path());
    let checkpoint_before = read_checkpoint(root.path());
    let head_before = git_head(root.path());

    let (release_output, release_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_release_after_state_commit")],
        &["skill", "release", "model-onboarding", "v1.0.0"],
    );

    assert!(
        !release_output.status.success(),
        "release unexpectedly succeeded"
    );
    assert_eq!(release_env["ok"], Value::Bool(false));
    assert_eq!(git_head(root.path()), head_before);
    assert!(
        !git_tag_exists(root.path(), "release/model-onboarding/v1.0.0"),
        "failed release should delete the release tag"
    );
    assert_eq!(read_operations_log(root.path()), operations_before);
    assert_eq!(read_checkpoint(root.path()), checkpoint_before);
    assert_eq!(git_status_short_for(root.path(), &["state/registry"]), "");
}

#[test]
fn skill_project_rejects_unmanaged_target_ownership() {
    let root = TestDir::new("registry-skill-project-observed");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/observed-claude");
    fs::create_dir_all(&target_path).expect("create observed target path");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "observed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_observed_claude",
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        None,
    );
    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert_eq!(
        project_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}

#[test]
fn skill_project_backs_up_existing_projection_path_before_replace() {
    let root = TestDir::new("registry-skill-project-backup");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    let (target_output, _) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        "target_claude_claude_project_a",
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let existing_projection = target_path.join("model-onboarding");
    fs::create_dir_all(&existing_projection).expect("create existing projection path");
    fs::write(
        existing_projection.join("legacy.txt"),
        "legacy projection content",
    )
    .expect("write legacy projection marker");

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        Some("copy"),
    );
    assert!(
        project_output.status.success(),
        "project failed: stderr={} stdout={}",
        String::from_utf8_lossy(&project_output.stderr),
        String::from_utf8_lossy(&project_output.stdout)
    );

    let backup_path = project_env["data"]["backup"]["backup_path"]
        .as_str()
        .expect("backup path should be returned");
    let backup_path = Path::new(backup_path);
    assert!(backup_path.exists(), "backup path should exist");
    assert!(
        backup_path.join("legacy.txt").exists(),
        "backup should preserve replaced content"
    );
}

#[test]
fn skill_project_rolls_back_projection_after_post_materialize_failure() {
    let root = TestDir::new("v3-skill-project-rollback");
    write_example_skill(root.path(), "model-onboarding");

    let (save_output, _) = save_skill(root.path(), "model-onboarding");
    assert!(save_output.status.success(), "save should succeed");

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

    let existing_projection = target_path.join("model-onboarding");
    fs::create_dir_all(&existing_projection).expect("create existing projection path");
    fs::write(
        existing_projection.join("legacy.txt"),
        "legacy projection content",
    )
    .expect("write legacy projection marker");

    let (project_output, project_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_project_after_materialize")],
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
            "--method",
            "copy",
        ],
    );

    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert!(
        existing_projection.join("legacy.txt").exists(),
        "legacy projection should be restored after failure"
    );
    assert!(
        !existing_projection.join("SKILL.md").exists(),
        "failed projection should not leave copied skill files"
    );

    let rules =
        fs::read_to_string(root.path().join("state/registry/rules.json")).expect("read rules");
    let projections = fs::read_to_string(root.path().join("state/registry/projections.json"))
        .expect("read projections");
    assert!(
        !rules.contains("model-onboarding"),
        "rules state should roll back"
    );
    assert!(
        !projections.contains("model-onboarding"),
        "projection state should roll back"
    );
}

#[test]
fn skill_project_eventstore_preflight_failure_blocks_mutation() {
    let root = TestDir::new("v3-skill-project-eventstore-preflight");
    write_example_skill(root.path(), "model-onboarding");

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

    let events_dir = root.path().join("state/events");
    fs::remove_dir_all(&events_dir).expect("remove command events dir");
    fs::write(&events_dir, "not a directory\n").expect("block command event dir");

    let (project_output, project_env) = run_loom_with_env(
        root.path(),
        &[],
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
            "--method",
            "copy",
        ],
    );

    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert_eq!(
        project_env["error"]["code"],
        Value::String("INTERNAL_ERROR".to_string())
    );
    assert!(
        !target_path.join("model-onboarding/SKILL.md").exists(),
        "projection should not be materialized when audit preflight fails"
    );

    let rules =
        fs::read_to_string(root.path().join("state/registry/rules.json")).expect("read rules");
    let projections = fs::read_to_string(root.path().join("state/registry/projections.json"))
        .expect("read projections");
    assert!(!rules.contains("model-onboarding"));
    assert!(!projections.contains("model-onboarding"));
}

#[test]
fn skill_project_terminal_audit_failure_reports_error_after_mutation() {
    let root = TestDir::new("v3-skill-project-eventstore-append");
    write_example_skill(root.path(), "model-onboarding");

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

    let (project_output, project_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "command_event_append_finished")],
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
            "--method",
            "copy",
        ],
    );

    assert!(
        !project_output.status.success(),
        "terminal audit failure should fail an audit-required command"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert_eq!(
        project_env["error"]["code"],
        Value::String("INTERNAL_ERROR".to_string())
    );
    assert!(
        target_path.join("model-onboarding/SKILL.md").exists(),
        "mutation completed before terminal audit failure and should remain materialized"
    );
}

#[test]
fn skill_project_rolls_back_operation_log_after_append_failure() {
    let root = TestDir::new("v3-skill-project-oplog-rollback");
    write_example_skill(root.path(), "model-onboarding");

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

    let existing_projection = target_path.join("model-onboarding");
    fs::create_dir_all(&existing_projection).expect("create existing projection path");
    fs::write(
        existing_projection.join("legacy.txt"),
        "legacy projection content",
    )
    .expect("write legacy projection marker");

    let operations_before = read_operations_log(root.path());
    let checkpoint_before = read_checkpoint(root.path());

    let (project_output, project_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_append")],
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
            "--method",
            "copy",
        ],
    );

    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert!(
        existing_projection.join("legacy.txt").exists(),
        "legacy projection should be restored after operation-log failure"
    );
    assert!(
        !existing_projection.join("SKILL.md").exists(),
        "failed projection should not leave copied skill files"
    );

    let rules =
        fs::read_to_string(root.path().join("state/registry/rules.json")).expect("read rules");
    let projections = fs::read_to_string(root.path().join("state/registry/projections.json"))
        .expect("read projections");
    assert!(!rules.contains("model-onboarding"));
    assert!(!projections.contains("model-onboarding"));
    assert_eq!(read_operations_log(root.path()), operations_before);
    assert_eq!(read_checkpoint(root.path()), checkpoint_before);
}

#[test]
fn skill_project_rolls_back_observation_after_late_audit_failure() {
    let root = TestDir::new("v3-skill-project-observation-rollback");
    write_example_skill(root.path(), "model-onboarding");

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

    let operations_before = read_operations_log(root.path());
    let checkpoint_before = read_checkpoint(root.path());

    let (project_output, project_env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "skill_project_after_observation")],
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
            "--method",
            "copy",
        ],
    );

    assert!(
        !project_output.status.success(),
        "project unexpectedly succeeded"
    );
    assert_eq!(project_env["ok"], Value::Bool(false));
    assert_eq!(read_operations_log(root.path()), operations_before);
    assert_eq!(read_checkpoint(root.path()), checkpoint_before);

    let observation_path = root
        .path()
        .join("state/registry/observations")
        .join("inst_model_onboarding_bind_claude_project_a_target_claude_claude_project_a.jsonl");
    assert!(
        !observation_path.exists(),
        "failed project should not leave observation history"
    );
}
