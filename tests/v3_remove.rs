use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use uuid::Uuid;

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("loom-{}-{}", prefix, Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn loom_bin() -> &'static str {
    env!("CARGO_BIN_EXE_loom")
}

fn run_loom(root: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(loom_bin())
        .arg("--json")
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

fn bootstrap_projected_skill(root: &Path) -> (String, String, String) {
    let skill_dir = root.join("skills/demo");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(skill_dir.join("SKILL.md"), "# Demo\n").expect("write skill");

    let target_path = root.join("live/claude-a");
    let target_path_str = target_path.to_string_lossy().to_string();
    let (_, target_env) = run_loom(
        root,
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            &target_path_str,
            "--ownership",
            "managed",
        ],
    );
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();

    let (_, binding_env) = run_loom(
        root,
        &[
            "workspace",
            "binding",
            "add",
            "--agent",
            "claude",
            "--profile",
            "default",
            "--matcher-kind",
            "path-prefix",
            "--matcher-value",
            &root.display().to_string(),
            "--target",
            &target_id,
        ],
    );
    let binding_id = binding_env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id")
        .to_string();

    let (_, project_env) = run_loom(
        root,
        &[
            "skill",
            "project",
            "demo",
            "--binding",
            &binding_id,
            "--method",
            "copy",
        ],
    );
    let instance_id = project_env["data"]["projection"]["instance_id"]
        .as_str()
        .expect("instance id")
        .to_string();

    (target_id, binding_id, instance_id)
}

#[test]
fn binding_remove_cascades_metadata_and_leaves_live_projection_in_place() {
    let root = TestDir::new("v3-binding-remove");
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
        env["data"]["removed_projections"].as_array().map(Vec::len),
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
        Some(0)
    );
}

#[test]
fn target_remove_rejects_referenced_target() {
    let root = TestDir::new("v3-target-remove-blocked");
    let (target_id, _binding_id, _instance_id) = bootstrap_projected_skill(root.path());

    let (output, env) = run_loom(root.path(), &["target", "remove", &target_id]);
    assert!(
        !output.status.success(),
        "target remove unexpectedly succeeded"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert!(
        env["error"]["details"]["binding_ids"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[test]
fn target_remove_succeeds_after_binding_metadata_is_cleared() {
    let root = TestDir::new("v3-target-remove-ok");
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
