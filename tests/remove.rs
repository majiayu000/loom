use std::fs;
use std::path::Path;

use serde_json::Value;

mod common;

use common::actions::{binding_add, skill_project, target_add};
use common::{TestDir, run_loom, run_loom_with_env, write_skill};

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
    let (target_id, binding_id, instance_id) = bootstrap_projected_skill(root.path());

    let live_projection = root.path().join("live/claude-a/demo/SKILL.md");
    assert!(
        live_projection.exists(),
        "projection should exist before remove"
    );
    let live_projection_dir = live_projection
        .parent()
        .expect("live projection parent")
        .canonicalize()
        .expect("canonicalize live projection parent");

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
        Value::String(live_projection_dir.display().to_string())
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

    let (orphan_list_output, orphan_list_env) = run_loom(root.path(), &["skill", "orphan", "list"]);
    assert!(
        orphan_list_output.status.success(),
        "orphan list failed: stderr={} stdout={}",
        String::from_utf8_lossy(&orphan_list_output.stderr),
        String::from_utf8_lossy(&orphan_list_output.stdout)
    );
    assert_eq!(orphan_list_env["ok"], Value::Bool(true));
    assert_eq!(
        orphan_list_env["cmd"],
        Value::String("skill.orphan.list".to_string())
    );
    assert_eq!(orphan_list_env["data"]["count"], Value::from(1));
    assert_eq!(
        orphan_list_env["data"]["orphaned_projection_ids"][0],
        Value::String(instance_id)
    );
    assert_eq!(
        orphan_list_env["data"]["orphaned_paths"][0],
        Value::String(live_projection_dir.display().to_string())
    );
    assert_eq!(
        orphan_list_env["data"]["projections"][0]["live_path_exists"],
        Value::Bool(true)
    );
    assert!(
        !orphan_list_env["meta"]
            .as_object()
            .is_some_and(|meta| meta.contains_key("op_id")),
        "read-only orphan list must not report an operation id"
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
fn orphan_clean_after_live_delete_audit_failure_does_not_restore_removed_projection_metadata() {
    let root = TestDir::new("registry-orphan-clean-delete-audit-failure");
    let (_target_id, binding_id, instance_id) = bootstrap_projected_skill(root.path());
    let live_projection_dir = root.path().join("live/claude-a/demo");
    assert!(
        live_projection_dir.exists(),
        "projection should exist before orphan cleanup"
    );

    let (binding_remove_output, _) = run_loom(
        root.path(),
        &["workspace", "binding", "remove", &binding_id],
    );
    assert!(
        binding_remove_output.status.success(),
        "binding remove should succeed first"
    );

    let operations_before =
        fs::read_to_string(root.path().join("state/registry/ops/operations.jsonl"))
            .expect("read operations before orphan clean");
    let checkpoint_before =
        fs::read_to_string(root.path().join("state/registry/ops/checkpoint.json"))
            .expect("read checkpoint before orphan clean");

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_append")],
        &["skill", "orphan", "clean", "--delete-live-paths"],
    );
    assert!(
        !output.status.success(),
        "orphan clean unexpectedly succeeded"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("leaving metadata removed")),
        "error should explain why projection metadata was not restored: {env}"
    );
    assert!(
        !live_projection_dir.exists(),
        "validated live path was already deleted and cannot be restored"
    );

    let projections = fs::read_to_string(root.path().join("state/registry/projections.json"))
        .expect("read projections after orphan clean failure");
    assert!(
        !projections.contains(&instance_id),
        "failed cleanup must not restore projection metadata for a deleted live path"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/ops/operations.jsonl"))
            .expect("read operations after orphan clean"),
        operations_before,
        "operation log append must still roll back"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/ops/checkpoint.json"))
            .expect("read checkpoint after orphan clean"),
        checkpoint_before,
        "checkpoint update must still roll back"
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
