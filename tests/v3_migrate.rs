use std::fs;

use serde_json::{Value, json};

mod common;

use common::{TestDir, legacy_target_payload, run_loom, write_legacy_targets};

#[test]
fn migrate_plan_surfaces_observed_target_candidates_from_v2() {
    let root = TestDir::new("v3-migrate-plan");
    let legacy_skill_path = root.path().join("live/claude/demo");
    fs::create_dir_all(&legacy_skill_path).expect("create live skill dir");

    write_legacy_targets(
        root.path(),
        legacy_target_payload(
            "symlink",
            Some(legacy_skill_path.display().to_string()),
            None,
        ),
    );

    let (output, env) = run_loom(root.path(), &["migrate", "v2-to-v3", "--plan"]);

    assert!(
        output.status.success(),
        "plan failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(
        env["data"]["migration"]["legacy_skill_count"],
        Value::from(1)
    );
    assert_eq!(
        env["data"]["migration"]["candidate_targets"][0]["path"],
        Value::String(root.path().join("live/claude").display().to_string())
    );
    assert_eq!(
        env["data"]["migration"]["candidate_targets"][0]["ownership"],
        Value::String("observed".to_string())
    );
    assert_eq!(
        env["data"]["migration"]["candidate_targets"][0]["source_skills"],
        json!(["demo"])
    );
    assert_eq!(env["data"]["migration"]["unresolved"], json!([]));
}

#[test]
fn migrate_apply_writes_v3_targets_and_keeps_v2_state() {
    let root = TestDir::new("v3-migrate-apply");
    let legacy_skill_path = root.path().join("live/codex/demo");
    fs::create_dir_all(&legacy_skill_path).expect("create live skill dir");

    write_legacy_targets(
        root.path(),
        legacy_target_payload("copy", None, Some(legacy_skill_path.display().to_string())),
    );

    let legacy_before = fs::read_to_string(root.path().join("state/targets.json"))
        .expect("read legacy targets before apply");
    let (output, env) = run_loom(root.path(), &["migrate", "v2-to-v3", "--apply"]);

    assert!(
        output.status.success(),
        "apply failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["noop"], Value::Bool(false));
    assert_eq!(
        env["meta"]["op_id"].as_str().map(|value| !value.is_empty()),
        Some(true)
    );

    let v3_targets: Value = serde_json::from_str(
        &fs::read_to_string(root.path().join("state/v3/targets.json")).expect("read v3 targets"),
    )
    .expect("parse v3 targets");
    assert_eq!(
        v3_targets["targets"][0]["path"],
        Value::String(root.path().join("live/codex").display().to_string())
    );
    assert_eq!(
        v3_targets["targets"][0]["ownership"],
        Value::String("observed".to_string())
    );

    let legacy_after = fs::read_to_string(root.path().join("state/targets.json"))
        .expect("read legacy targets after apply");
    assert_eq!(
        legacy_before, legacy_after,
        "v2 targets must remain untouched"
    );
}

#[test]
fn migrate_apply_refuses_unresolved_relative_legacy_paths() {
    let root = TestDir::new("v3-migrate-unresolved");
    write_legacy_targets(
        root.path(),
        json!({
            "skills": {
                "demo": {
                    "method": "symlink",
                    "claude_path": "relative/demo",
                    "codex_path": null
                }
            }
        }),
    );

    let (plan_output, plan_env) = run_loom(root.path(), &["migrate", "v2-to-v3", "--plan"]);
    assert!(plan_output.status.success(), "plan should succeed");
    assert_eq!(
        plan_env["data"]["migration"]["unresolved"][0]["reason"],
        Value::String("relative_path".to_string())
    );

    let (apply_output, apply_env) = run_loom(root.path(), &["migrate", "v2-to-v3", "--apply"]);
    assert!(
        !apply_output.status.success(),
        "apply unexpectedly succeeded"
    );
    assert_eq!(
        apply_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert_eq!(
        apply_env["error"]["details"]["migration"]["unresolved"][0]["reason"],
        Value::String("relative_path".to_string())
    );
}
