use std::path::Path;

use serde_json::Value;

mod common;

use common::actions::{binding_add, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};

fn bootstrap_projected_skill(root: &Path) -> (String, String, String) {
    write_skill(root, "demo", "# Demo\n");

    let target_path = root.join("live/claude-a");
    let (_, target_env) = target_add(root, "claude", &target_path, "managed");
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();

    let (_, binding_env) = binding_add(
        root,
        "claude",
        "default",
        "path-prefix",
        &root.display().to_string(),
        &target_id,
    );
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id")
        .to_string();

    let (_, project_env) = skill_project(root, "demo", &binding_id, Some("copy"));
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("instance id")
        .to_string();

    (target_id, binding_id, instance_id)
}

#[test]
fn binding_remove_cascades_metadata_and_leaves_live_projection_in_place() {
    let root = TestDir::new("registry-binding-remove");
    let (target_id, binding_id, _instance_id) = bootstrap_projected_skill(root.path());

    let live_projection = root.path().join("live/claude-a/demo/SKILL.md");
    assert!(
        live_projection.exists(),
        "projection should exist before remove"
    );

    let (output, env) = run_loom(
        root.path(),
        &["workspace", "binding", "remove", &binding_id],
    );
    assert!(
        output.status.success(),
        "binding remove failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["removed_rules"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        env["data"]["orphaned_projections"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        env["data"]["orphaned_paths"][0],
        Value::String(root.path().join("live/claude-a/demo").display().to_string())
    );
    assert!(
        env["meta"]["warnings"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    assert!(
        live_projection.exists(),
        "live projection must be left in place"
    );

    let (_, binding_list_env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert_eq!(binding_list_env["data"]["count"], Value::from(0));

    let (_, target_show_env) = run_loom(root.path(), &["target", "show", &target_id]);
    assert_eq!(
        target_show_env["data"]["bindings"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        target_show_env["data"]["rules"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        target_show_env["data"]["projections"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "orphaned projection should still be visible under the target"
    );
}

#[test]
fn target_remove_rejects_referenced_target() {
    let root = TestDir::new("registry-target-remove-blocked");
    let (target_id, _binding_id, _instance_id) = bootstrap_projected_skill(root.path());

    let (output, env) = run_loom(root.path(), &["target", "remove", &target_id]);
    assert!(
        !output.status.success(),
        "target remove unexpectedly succeeded"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("DEPENDENCY_CONFLICT".to_string())
    );
    assert!(
        env["error"]["details"]["binding_ids"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[test]
fn target_remove_succeeds_after_binding_metadata_is_cleared() {
    let root = TestDir::new("registry-target-remove-ok");
    let (target_id, binding_id, _instance_id) = bootstrap_projected_skill(root.path());

    let (binding_remove_output, _) = run_loom(
        root.path(),
        &["workspace", "binding", "remove", &binding_id],
    );
    assert!(
        binding_remove_output.status.success(),
        "binding remove should succeed first"
    );

    let (output, env) = run_loom(root.path(), &["target", "remove", &target_id]);
    assert!(
        output.status.success(),
        "target remove failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["target"]["target_id"], Value::String(target_id));

    let (_, target_list_env) = run_loom(root.path(), &["target", "list"]);
    assert_eq!(target_list_env["data"]["count"], Value::from(0));
}
