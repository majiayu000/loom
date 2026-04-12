use std::fs;
use std::path::Path;

use serde_json::Value;

mod common;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};

#[test]
fn skill_capture_copies_live_projection_back_into_source_and_commits() {
    let root = TestDir::new("v3-capture");
    write_skill(
        root.path(),
        "model-onboarding",
        "# model-onboarding\n\nsource v1\n",
    );

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
    assert!(
        skill_project(
            root.path(),
            "model-onboarding",
            "bind_claude_project_a",
            Some("copy"),
        )
        .0
        .status
        .success()
    );

    let live_file = target_path.join("model-onboarding").join("SKILL.md");
    fs::write(
        &live_file,
        "# model-onboarding\n\ncaptured from live copy\n",
    )
    .expect("edit live projection");

    let (capture_output, capture_env) = run_loom(
        root.path(),
        &[
            "skill",
            "capture",
            "model-onboarding",
            "--binding",
            "bind_claude_project_a",
        ],
    );
    assert!(
        capture_output.status.success(),
        "capture failed: stderr={} stdout={}",
        String::from_utf8_lossy(&capture_output.stderr),
        String::from_utf8_lossy(&capture_output.stdout)
    );
    assert_eq!(capture_env["ok"], Value::Bool(true));
    assert_eq!(
        capture_env["data"]["capture"]["instance_id"],
        Value::String(
            "inst_model_onboarding_bind_claude_project_a_target_claude_claude_project_a"
                .to_string()
        )
    );
    assert_eq!(capture_env["data"]["capture"]["noop"], Value::Bool(false));
    assert_eq!(
        capture_env["data"]["capture"]["commit"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );
    assert_eq!(
        capture_env["meta"]["op_id"]
            .as_str()
            .map(|value| !value.is_empty()),
        Some(true)
    );

    let backup_path = capture_env["data"]["capture"]["backup"]["backup_path"]
        .as_str()
        .expect("capture backup path should be returned");
    let backup_path = Path::new(backup_path);
    assert!(backup_path.exists(), "capture backup path should exist");
    let backup_body =
        fs::read_to_string(backup_path.join("SKILL.md")).expect("read captured backup source");
    assert!(backup_body.contains("source v1"));

    let source_file = root.path().join("skills/model-onboarding/SKILL.md");
    let source_body = fs::read_to_string(source_file).expect("read source skill");
    assert!(source_body.contains("captured from live copy"));
}

#[test]
fn skill_capture_requires_explicit_selector() {
    let root = TestDir::new("v3-capture-selector");
    let (output, env) = run_loom(root.path(), &["skill", "capture"]);
    assert!(!output.status.success(), "capture unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}
