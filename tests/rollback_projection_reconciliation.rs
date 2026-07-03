use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, run_loom_with_env, write_skill};

struct RollbackProjectionFixture {
    root: TestDir,
    projection_path: PathBuf,
    instance_id: String,
    binding_id: String,
    target_id: String,
}

fn rollback_projection_fixture(method: &str) -> RollbackProjectionFixture {
    let root = TestDir::new(&format!("registry-skill-rollback-reconcile-{method}"));
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

    let target_path = root.path().join(format!("live/claude-{method}"));
    let (target_output, target_env) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();
    let (binding_output, binding_env) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        &format!("/tmp/project-{method}"),
        &target_id,
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id")
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
    let (project_output, project_env) =
        skill_project(root.path(), "model-onboarding", &binding_id, Some(method));
    assert!(project_output.status.success(), "project should succeed");
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("projection instance id")
        .to_string();
    let projection_path = PathBuf::from(
        project_env["data"]["projection"]["materialized_path"]
            .as_str()
            .expect("projection path"),
    );
    assert!(
        fs::read_to_string(projection_path.join("SKILL.md"))
            .expect("read live projection")
            .contains("source v2"),
        "fixture should project source v2 before rollback"
    );

    RollbackProjectionFixture {
        root,
        projection_path,
        instance_id,
        binding_id,
        target_id,
    }
}

fn assert_meta_warning_contains(env: &Value, needle: &str) {
    let warnings = env["meta"]["warnings"]
        .as_array()
        .expect("meta warnings array");
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().is_some_and(|text| text.contains(needle))),
        "expected warning containing {needle:?}: {env}"
    );
}

fn git_output(root: &Path, args: &[&str]) -> String {
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

fn commit_skill_without_registry(root: &Path, skill: &str, body: &str, message: &str) {
    if !root.join(".git").exists() {
        git_output(root, &["init"]);
    }
    write_skill(root, skill, body);
    let skill_rel = format!("skills/{skill}");
    git_output(root, &["add", "--", &skill_rel]);
    git_output(root, &["commit", "-m", message, "--", &skill_rel]);
    assert!(
        !root.join("state/registry").exists(),
        "test helper should not initialize registry state"
    );
}

#[test]
fn rollback_noop_reports_reconciliation_shape_without_registry_init() {
    let root = TestDir::new("registry-skill-rollback-noop-reconciliation");
    commit_skill_without_registry(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
        "seed skill",
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
    assert_eq!(rollback_env["data"]["source_restored"], Value::Bool(false));
    assert_eq!(
        rollback_env["data"]["registry_restored"],
        Value::Bool(false)
    );
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(true)
    );
    assert_eq!(
        rollback_env["data"]["projection_reconciliation"]["status"],
        Value::String("noop".to_string())
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "noop rollback should not initialize registry state"
    );
}

#[test]
fn rollback_without_existing_registry_reports_unverified_projection_reconciliation() {
    let root = TestDir::new("registry-skill-rollback-no-registry-reconciliation");
    commit_skill_without_registry(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
        "seed skill",
    );
    commit_skill_without_registry(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v2\n",
        "update skill",
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
    assert_eq!(rollback_env["data"]["noop"], Value::Bool(false));
    assert_eq!(rollback_env["data"]["source_restored"], Value::Bool(true));
    assert_eq!(rollback_env["data"]["registry_restored"], Value::Bool(true));
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    assert_eq!(
        rollback_env["data"]["projection_reconciliation"]["status"],
        Value::String("registry_missing".to_string())
    );
    assert_eq!(
        rollback_env["data"]["projection_reconciliation"]["next_actions"][0]["type"],
        Value::String("manual_review_required".to_string())
    );
    assert_meta_warning_contains(
        &rollback_env,
        "registry state was not initialized before rollback",
    );
    let source = fs::read_to_string(root.path().join("skills/model-onboarding/SKILL.md"))
        .expect("read source skill");
    assert!(source.contains("source v1"));
    assert!(!source.contains("source v2"));
    assert!(
        root.path().join("state/registry").exists(),
        "non-noop rollback should record registry audit state"
    );
}

#[test]
fn rollback_reports_copy_and_materialize_projection_reapply_plan() {
    for method in ["copy", "materialize"] {
        let fixture = rollback_projection_fixture(method);

        let (rollback_output, rollback_env) = run_loom(
            fixture.root.path(),
            &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
        );

        assert!(
            rollback_output.status.success(),
            "rollback failed for {method}: stderr={} stdout={}",
            String::from_utf8_lossy(&rollback_output.stderr),
            String::from_utf8_lossy(&rollback_output.stdout)
        );
        assert_eq!(rollback_env["ok"], Value::Bool(true));
        assert_eq!(rollback_env["data"]["source_restored"], Value::Bool(true));
        assert_eq!(rollback_env["data"]["registry_restored"], Value::Bool(true));
        assert_eq!(
            rollback_env["data"]["live_projection_reconciled"],
            Value::Bool(false)
        );

        let source =
            fs::read_to_string(fixture.root.path().join("skills/model-onboarding/SKILL.md"))
                .expect("read source skill");
        assert!(source.contains("source v1"));
        assert!(!source.contains("source v2"));
        let live = fs::read_to_string(fixture.projection_path.join("SKILL.md"))
            .expect("read live projection");
        assert!(live.contains("source v2"));
        assert!(!live.contains("source v1"));

        let reconciliation = &rollback_env["data"]["projection_reconciliation"];
        assert_eq!(
            reconciliation["status"],
            Value::String("requires_reapply".to_string())
        );
        assert_eq!(
            reconciliation["mode"],
            Value::String("recovery_plan_only".to_string())
        );
        assert_eq!(
            reconciliation["requires_projection_reapply"],
            Value::Bool(true)
        );
        assert_eq!(
            reconciliation["live_projection_reconciled"],
            Value::Bool(false)
        );
        let item = &reconciliation["items"][0];
        assert_eq!(
            item["instance_id"],
            Value::String(fixture.instance_id.clone())
        );
        assert_eq!(
            item["skill_id"],
            Value::String("model-onboarding".to_string())
        );
        assert_eq!(
            item["binding_id"],
            Value::String(fixture.binding_id.clone())
        );
        assert_eq!(item["target_id"], Value::String(fixture.target_id.clone()));
        assert_eq!(
            item["materialized_path"],
            Value::String(fixture.projection_path.to_string_lossy().into_owned())
        );
        assert_eq!(item["method"], Value::String(method.to_string()));
        assert_eq!(
            item["status"],
            Value::String("requires_reapply".to_string())
        );
        assert_eq!(item["live_path_exists"], Value::Bool(true));
        assert_eq!(item["requires_projection_reapply"], Value::Bool(true));

        let command = item["next_action"]["command"]
            .as_str()
            .expect("next action command");
        assert!(
            command.starts_with("loom --json --root "),
            "unexpected command: {command}"
        );
        assert!(
            command.contains(" skill project model-onboarding "),
            "unexpected command: {command}"
        );
        assert!(
            command.contains(&format!(" --binding {} ", fixture.binding_id)),
            "unexpected command: {command}"
        );
        assert!(
            command.contains(&format!(" --target {} ", fixture.target_id)),
            "unexpected command: {command}"
        );
        assert!(
            command.ends_with(&format!(" --method {method}")),
            "unexpected command: {command}"
        );
        assert_eq!(
            reconciliation["next_actions"][0]["command"],
            Value::String(command.to_string())
        );
        assert_meta_warning_contains(&rollback_env, "live projections require reapply");
        assert_meta_warning_contains(&rollback_env, &fixture.instance_id);

        let (reproject_output, _reproject_env) = run_loom(
            fixture.root.path(),
            &[
                "skill",
                "project",
                "model-onboarding",
                "--binding",
                &fixture.binding_id,
                "--target",
                &fixture.target_id,
                "--method",
                method,
            ],
        );
        assert!(
            reproject_output.status.success(),
            "reproject command failed for {method}: stderr={} stdout={}",
            String::from_utf8_lossy(&reproject_output.stderr),
            String::from_utf8_lossy(&reproject_output.stdout)
        );
        let live = fs::read_to_string(fixture.projection_path.join("SKILL.md"))
            .expect("read reprojected live projection");
        assert!(live.contains("source v1"));
        assert!(!live.contains("source v2"));
    }
}

#[test]
fn rollback_reports_missing_projection_reapply_plan() {
    let fixture = rollback_projection_fixture("copy");
    let target_path = fixture
        .projection_path
        .parent()
        .expect("projection target path")
        .to_path_buf();
    fs::remove_dir_all(&target_path).expect("remove live target path");

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    let item = &rollback_env["data"]["projection_reconciliation"]["items"][0];
    assert_eq!(
        item["status"],
        Value::String("missing_projection_path".to_string())
    );
    assert_eq!(item["live_path_exists"], Value::Bool(false));
    assert_eq!(item["requires_projection_reapply"], Value::Bool(true));
    assert_eq!(
        item["next_action"]["type"],
        Value::String("command".to_string())
    );
    assert!(
        item["next_action"]["command"]
            .as_str()
            .expect("next action command")
            .contains(" skill project model-onboarding "),
        "expected projection recovery command: {rollback_env}"
    );
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    assert_meta_warning_contains(&rollback_env, "live projections require reapply");
}

#[test]
fn rollback_reports_missing_symlink_projection_reapply_plan() {
    let fixture = rollback_projection_fixture("symlink");
    fs::remove_file(&fixture.projection_path).expect("remove symlink projection path");

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    assert_eq!(
        reconciliation["status"],
        Value::String("requires_reapply".to_string())
    );
    assert_eq!(
        reconciliation["requires_projection_reapply"],
        Value::Bool(true)
    );
    let item = &reconciliation["items"][0];
    assert_eq!(item["method"], Value::String("symlink".to_string()));
    assert_eq!(
        item["status"],
        Value::String("missing_projection_path".to_string())
    );
    assert_eq!(item["live_path_exists"], Value::Bool(false));
    assert_eq!(item["requires_projection_reapply"], Value::Bool(true));
    let command = item["next_action"]["command"]
        .as_str()
        .expect("next action command");
    assert!(
        command.ends_with(" --method symlink"),
        "unexpected command: {command}"
    );
    assert_meta_warning_contains(&rollback_env, "live projections require reapply");

    let (reproject_output, _reproject_env) = run_loom(
        fixture.root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &fixture.binding_id,
            "--target",
            &fixture.target_id,
            "--method",
            "symlink",
        ],
    );
    assert!(
        reproject_output.status.success(),
        "reproject command failed: stderr={} stdout={}",
        String::from_utf8_lossy(&reproject_output.stderr),
        String::from_utf8_lossy(&reproject_output.stdout)
    );
    let live = fs::read_to_string(fixture.projection_path.join("SKILL.md"))
        .expect("read reprojected symlink projection");
    assert!(live.contains("source v1"));
    assert!(!live.contains("source v2"));
}

#[cfg(unix)]
#[test]
fn rollback_reports_dangling_symlink_projection_reapply_plan() {
    let fixture = rollback_projection_fixture("symlink");
    fs::remove_file(&fixture.projection_path).expect("remove symlink projection path");
    std::os::unix::fs::symlink(
        fixture.root.path().join("missing-symlink-target"),
        &fixture.projection_path,
    )
    .expect("create dangling symlink projection path");
    assert!(
        fs::symlink_metadata(&fixture.projection_path).is_ok(),
        "dangling symlink itself should exist"
    );
    assert!(
        fs::metadata(&fixture.projection_path).is_err(),
        "dangling symlink target should be missing"
    );

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        reconciliation["status"],
        Value::String("requires_reapply".to_string())
    );
    let item = &reconciliation["items"][0];
    assert_eq!(item["method"], Value::String("symlink".to_string()));
    assert_eq!(
        item["status"],
        Value::String("missing_projection_path".to_string())
    );
    assert_eq!(item["live_path_exists"], Value::Bool(false));
    assert_eq!(item["requires_projection_reapply"], Value::Bool(true));
    assert_eq!(
        item["next_action"]["type"],
        Value::String("command".to_string())
    );
    assert_meta_warning_contains(&rollback_env, "live projections require reapply");
}

#[cfg(unix)]
#[test]
fn rollback_reports_wrong_target_symlink_projection_reapply_plan() {
    let fixture = rollback_projection_fixture("symlink");
    fs::remove_file(&fixture.projection_path).expect("remove symlink projection path");
    let wrong_target = fixture.root.path().join("wrong-symlink-target");
    fs::create_dir_all(&wrong_target).expect("create wrong target");
    fs::write(wrong_target.join("SKILL.md"), "# wrong\n").expect("write wrong target");
    std::os::unix::fs::symlink(&wrong_target, &fixture.projection_path)
        .expect("create wrong-target symlink projection path");

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    let item = &rollback_env["data"]["projection_reconciliation"]["items"][0];
    assert_eq!(item["method"], Value::String("symlink".to_string()));
    assert_eq!(
        item["status"],
        Value::String("symlink_target_mismatch".to_string())
    );
    assert_eq!(item["live_path_exists"], Value::Bool(true));
    assert_eq!(item["requires_projection_reapply"], Value::Bool(true));
    assert_eq!(
        item["next_action"]["type"],
        Value::String("command".to_string())
    );

    let (reproject_output, _reproject_env) = run_loom(
        fixture.root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &fixture.binding_id,
            "--target",
            &fixture.target_id,
            "--method",
            "symlink",
        ],
    );
    assert!(
        reproject_output.status.success(),
        "reproject command failed: stderr={} stdout={}",
        String::from_utf8_lossy(&reproject_output.stderr),
        String::from_utf8_lossy(&reproject_output.stdout)
    );
    let live = fs::read_to_string(fixture.projection_path.join("SKILL.md"))
        .expect("read reprojected symlink projection");
    assert!(live.contains("source v1"));
    assert!(!live.contains("wrong"));
}

#[cfg(unix)]
#[test]
fn rollback_reports_non_symlink_projection_path_reapply_plan() {
    let fixture = rollback_projection_fixture("symlink");
    fs::remove_file(&fixture.projection_path).expect("remove symlink projection path");
    fs::create_dir_all(&fixture.projection_path).expect("replace projection with directory");
    fs::write(
        fixture.projection_path.join("SKILL.md"),
        "# stale directory\n",
    )
    .expect("write stale directory projection");

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    let item = &rollback_env["data"]["projection_reconciliation"]["items"][0];
    assert_eq!(item["method"], Value::String("symlink".to_string()));
    assert_eq!(item["status"], Value::String("not_symlink".to_string()));
    assert_eq!(item["live_path_exists"], Value::Bool(true));
    assert_eq!(item["requires_projection_reapply"], Value::Bool(true));
    assert_eq!(
        item["next_action"]["type"],
        Value::String("command".to_string())
    );

    let (reproject_output, _reproject_env) = run_loom(
        fixture.root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            &fixture.binding_id,
            "--target",
            &fixture.target_id,
            "--method",
            "symlink",
        ],
    );
    assert!(
        reproject_output.status.success(),
        "reproject command failed: stderr={} stdout={}",
        String::from_utf8_lossy(&reproject_output.stderr),
        String::from_utf8_lossy(&reproject_output.stdout)
    );
    assert!(
        fs::symlink_metadata(&fixture.projection_path)
            .expect("reprojected path metadata")
            .file_type()
            .is_symlink(),
        "reproject should restore symlink projection"
    );
    let live = fs::read_to_string(fixture.projection_path.join("SKILL.md"))
        .expect("read reprojected symlink projection");
    assert!(live.contains("source v1"));
    assert!(!live.contains("stale directory"));
}

#[test]
fn rollback_marks_symlink_projection_reconciled_without_reapply() {
    let fixture = rollback_projection_fixture("symlink");

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    let source = fs::read_to_string(fixture.root.path().join("skills/model-onboarding/SKILL.md"))
        .expect("read source skill");
    assert!(source.contains("source v1"));
    let live = fs::read_to_string(fixture.projection_path.join("SKILL.md"))
        .expect("read symlink projection");
    assert!(live.contains("source v1"));
    assert!(!live.contains("source v2"));

    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        reconciliation["status"],
        Value::String("verified_no_reapply_required".to_string())
    );
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(true)
    );
    assert_eq!(
        reconciliation["live_projection_reconciled"],
        Value::Bool(true)
    );
    assert_eq!(
        reconciliation["requires_projection_reapply"],
        Value::Bool(false)
    );
    assert_eq!(
        reconciliation["next_actions"]
            .as_array()
            .expect("next actions")
            .len(),
        0
    );
    let item = &reconciliation["items"][0];
    assert_eq!(item["method"], Value::String("symlink".to_string()));
    assert_eq!(item["status"], Value::String("symlink_noop".to_string()));
    assert_eq!(item["live_path_exists"], Value::Bool(true));
    assert_eq!(item["requires_projection_reapply"], Value::Bool(false));
    assert_eq!(item["next_action"], Value::Null);
}

#[test]
fn rollback_surfaces_projection_snapshot_load_failure() {
    let fixture = rollback_projection_fixture("copy");

    let (rollback_output, rollback_env) = run_loom_with_env(
        fixture.root.path(),
        &[(
            "LOOM_ROLLBACK_FAULT_INJECT",
            "projection_reconciliation_snapshot_load",
        )],
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(rollback_env["ok"], Value::Bool(true));
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        reconciliation["status"],
        Value::String("registry_unavailable".to_string())
    );
    assert_eq!(
        reconciliation["error"]["code"],
        Value::String("REGISTRY_STATE_UNAVAILABLE".to_string())
    );
    assert_eq!(
        reconciliation["next_actions"][0]["type"],
        Value::String("manual_review_required".to_string())
    );
    assert_meta_warning_contains(&rollback_env, "registry snapshot loading failed");
}

#[test]
fn rollback_surfaces_real_projection_snapshot_load_failure() {
    let fixture = rollback_projection_fixture("copy");
    fs::write(
        fixture.root.path().join("state/registry/projections.json"),
        "not json",
    )
    .expect("corrupt projections snapshot");

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "model-onboarding", "--to", "HEAD~2"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(rollback_env["ok"], Value::Bool(true));
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        reconciliation["status"],
        Value::String("registry_unavailable".to_string())
    );
    assert_eq!(
        reconciliation["error"]["code"],
        Value::String("REGISTRY_STATE_UNAVAILABLE".to_string())
    );
    assert_meta_warning_contains(
        &rollback_env,
        "could not record projection observations because registry snapshot loading failed",
    );
    assert_meta_warning_contains(&rollback_env, "registry snapshot loading failed");
}
