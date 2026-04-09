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

fn write_skill(root: &Path, skill: &str) {
    let skill_dir = root.join("skills").join(skill);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("# {}\n\nexample skill\n", skill),
    )
    .expect("write skill file");
}

#[test]
fn skill_project_creates_projection_rule_and_instance() {
    let root = TestDir::new("v3-skill-project");
    write_skill(root.path(), "model-onboarding");

    let (save_output, _) = run_loom(root.path(), &["skill", "save", "model-onboarding"]);
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/claude-project-a");
    let target_path_str = target_path.to_string_lossy().to_string();
    let (target_output, _) = run_loom(
        root.path(),
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
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = run_loom(
        root.path(),
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
            "/tmp/project-a",
            "--target",
            "target_claude_claude_project_a",
        ],
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
        ],
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
fn skill_project_rejects_unmanaged_target_ownership() {
    let root = TestDir::new("v3-skill-project-observed");
    write_skill(root.path(), "model-onboarding");

    let (save_output, _) = run_loom(root.path(), &["skill", "save", "model-onboarding"]);
    assert!(save_output.status.success(), "save should succeed");

    let target_path = root.path().join("live/observed-claude");
    fs::create_dir_all(&target_path).expect("create observed target path");
    let target_path_str = target_path.to_string_lossy().to_string();
    let (target_output, _) = run_loom(
        root.path(),
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            &target_path_str,
            "--ownership",
            "observed",
        ],
    );
    assert!(target_output.status.success(), "target add should succeed");

    let (binding_output, _) = run_loom(
        root.path(),
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
            "/tmp/project-a",
            "--target",
            "target_claude_observed_claude",
        ],
    );
    assert!(
        binding_output.status.success(),
        "binding add should succeed"
    );

    let (project_output, project_env) = run_loom(
        root.path(),
        &[
            "skill",
            "project",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
        ],
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
