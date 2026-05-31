mod common;

use std::fs;

use common::actions::save_skill;
use common::{TestDir, run_loom, write_skill};
use serde_json::Value;

#[test]
fn skill_verify_matches_after_save() {
    let root = TestDir::new("skill-verify-match");
    write_skill(root.path(), "demo", "# demo\n\nbody v1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());

    let (output, env) = run_loom(root.path(), &["skill", "verify", "demo"]);
    assert!(output.status.success(), "verify should succeed");
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["skill"], Value::String("demo".to_string()));
    assert_eq!(env["data"]["matches"], Value::Bool(true));
    assert_eq!(
        env["data"]["drifted_paths"].as_array().map(Vec::len),
        Some(0)
    );
    assert!(
        env["data"]["head_tree_oid"].is_string(),
        "head_tree_oid must be populated after save"
    );
    assert!(
        env["data"]["last_source_commit"].is_string(),
        "last_source_commit must be populated after save"
    );
}

#[test]
fn skill_verify_detects_drift_after_external_edit() {
    let root = TestDir::new("skill-verify-drift");
    write_skill(root.path(), "demo", "# demo\n\nbody v1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());

    // External edit that bypasses `skill save`.
    fs::write(
        root.path().join("skills/demo/SKILL.md"),
        "# demo\n\nbody v2 (drifted)\n",
    )
    .expect("overwrite skill body");

    let (output, env) = run_loom(root.path(), &["skill", "verify", "demo"]);
    assert!(output.status.success(), "verify should still succeed");
    assert_eq!(env["data"]["matches"], Value::Bool(false));
    let drifted = env["data"]["drifted_paths"]
        .as_array()
        .expect("drifted_paths array");
    assert_eq!(
        drifted.len(),
        1,
        "exactly one modified path expected, got {drifted:?}"
    );
    let drift_entry = drifted[0].as_str().expect("drift path string");
    assert!(
        drift_entry.contains("skills/demo/SKILL.md"),
        "expected drifted entry to reference SKILL.md, got {drift_entry}"
    );
}

#[test]
fn skill_verify_detects_untracked_file_drift() {
    let root = TestDir::new("skill-verify-untracked");
    write_skill(root.path(), "demo", "# demo\n\nbody v1\n");
    assert!(save_skill(root.path(), "demo").0.status.success());

    // New file dropped into the skill directory without a save.
    fs::write(
        root.path().join("skills/demo/NOTES.md"),
        "side notes not yet committed\n",
    )
    .expect("write untracked note");

    let (output, env) = run_loom(root.path(), &["skill", "verify", "demo"]);
    assert!(output.status.success());
    assert_eq!(env["data"]["matches"], Value::Bool(false));
    let drifted = env["data"]["drifted_paths"]
        .as_array()
        .expect("drifted_paths array");
    assert!(
        drifted
            .iter()
            .any(|p| p.as_str().unwrap_or("").contains("NOTES.md")),
        "expected untracked NOTES.md to appear, got {drifted:?}"
    );
}

#[test]
fn skill_verify_reports_skill_not_found() {
    let root = TestDir::new("skill-verify-missing");
    let (output, env) = run_loom(root.path(), &["skill", "verify", "ghost"]);
    assert!(
        !output.status.success(),
        "verify on missing skill should fail"
    );
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"].as_str(),
        Some("SKILL_NOT_FOUND"),
        "unexpected error: {:?}",
        env["error"]
    );
}
