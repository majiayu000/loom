use std::fs;
use std::path::PathBuf;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};

struct NoopProjectionFixture {
    root: TestDir,
    projection_path: PathBuf,
}

fn noop_projection_fixture() -> NoopProjectionFixture {
    let root = TestDir::new("rollback-noop-projection-reconciliation");
    write_skill(root.path(), "demo", "# demo\n\nsource v1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());

    let target_path = root.path().join("live/claude-copy");
    let (target_output, target_env) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(target_output.status.success(), "target add should succeed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let (binding_output, binding_env) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/noop-projection",
        target_id,
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (project_output, project_env) =
        skill_project(root.path(), "demo", binding_id, Some("copy"));
    assert!(project_output.status.success(), "project should succeed");
    let projection_path = PathBuf::from(
        project_env["data"]["projection"]["materialized_path"]
            .as_str()
            .expect("projection path"),
    );
    assert!(
        fs::read_to_string(projection_path.join("SKILL.md"))
            .expect("read live projection")
            .contains("source v1"),
        "fixture should project source v1 before no-op rollback"
    );
    NoopProjectionFixture {
        root,
        projection_path,
    }
}

#[test]
fn rollback_noop_checks_existing_copy_projection_before_claiming_reconciled() {
    let fixture = noop_projection_fixture();
    write_skill(fixture.root.path(), "demo", "# demo\n\nsource v2\n");
    assert!(save_skill(fixture.root.path(), "demo").0.status.success());

    let (rollback_output, rollback_env) = run_loom(
        fixture.root.path(),
        &["skill", "rollback", "demo", "--to", "HEAD"],
    );

    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(rollback_env["ok"], Value::Bool(true));
    assert_eq!(rollback_env["data"]["noop"], Value::Bool(true));
    assert_eq!(rollback_env["data"]["source_restored"], Value::Bool(false));
    assert_eq!(
        rollback_env["data"]["live_projection_reconciled"],
        Value::Bool(false)
    );
    let reconciliation = &rollback_env["data"]["projection_reconciliation"];
    assert_eq!(
        reconciliation["status"],
        Value::String("requires_reapply".to_string())
    );
    assert_eq!(
        reconciliation["requires_projection_reapply"],
        Value::Bool(true)
    );
    assert_eq!(
        reconciliation["items"][0]["materialized_path"],
        Value::String(fixture.projection_path.to_string_lossy().into_owned())
    );
    assert_eq!(
        reconciliation["items"][0]["next_action"]["type"],
        Value::String("command".to_string())
    );
    assert!(
        reconciliation["items"][0]["next_action"]["command"]
            .as_str()
            .expect("recovery command")
            .contains(" skill project demo "),
        "expected raw project recovery command: {rollback_env}"
    );
    assert!(
        rollback_env["meta"]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|text| text.contains("live projections require reapply"))),
        "expected stale projection warning: {rollback_env}"
    );
}
