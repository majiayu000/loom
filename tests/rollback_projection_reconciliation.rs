use std::fs;
use std::path::PathBuf;

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
        assert_meta_warning_contains(
            &rollback_env,
            "did not update live copy/materialize projections",
        );
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
    assert_meta_warning_contains(
        &rollback_env,
        "did not update live copy/materialize projections",
    );
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
