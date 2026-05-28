use std::fs;

use serde_json::Value;

mod common;

use common::actions::save_skill;
use common::{TestDir, operations_log, run_loom, write_skill};

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn skill_trash_add_lists_and_restores_latest_entry() {
    let root = TestDir::new("skill-trash-restore");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");

    let (trash_output, trash_env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");
    assert_eq!(trash_env["ok"], Value::Bool(true));
    let trash_id = trash_env["data"]["trash_id"]
        .as_str()
        .expect("trash id")
        .to_string();
    assert!(!root.path().join("skills/demo").exists());
    assert!(
        root.path()
            .join("trash")
            .join(&trash_id)
            .join("skill/SKILL.md")
            .exists()
    );
    assert!(
        root.path()
            .join("trash")
            .join(&trash_id)
            .join("metadata.json")
            .exists()
    );

    let (list_output, list_env) = run_loom(root.path(), &["skill", "trash", "list"]);
    assert_success(&list_output, "trash list");
    assert_eq!(list_env["ok"], Value::Bool(true));
    assert_eq!(
        list_env["data"]["items"][0]["trash_id"],
        Value::String(trash_id.clone())
    );
    assert_eq!(
        list_env["data"]["items"][0]["skill"],
        Value::String("demo".to_string())
    );
    assert!(
        !list_env["meta"]
            .as_object()
            .is_some_and(|meta| meta.contains_key("op_id")),
        "read-only trash list must not report an op_id"
    );

    let (restore_output, restore_env) =
        run_loom(root.path(), &["skill", "trash", "restore", "demo"]);
    assert_success(&restore_output, "trash restore");
    assert_eq!(restore_env["ok"], Value::Bool(true));
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(!root.path().join("trash").join(&trash_id).exists());
    let operations = operations_log(root.path());
    assert!(operations.contains(r#""intent":"skill.trash.add""#));
    assert!(operations.contains(r#""intent":"skill.trash.restore""#));
}

#[test]
fn skill_trash_restore_refuses_to_overwrite_existing_skill() {
    let root = TestDir::new("skill-trash-restore-conflict");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");
    let (trash_output, _) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");

    write_skill(root.path(), "demo", "# Demo\n\nreplacement\n");
    let (restore_output, restore_env) =
        run_loom(root.path(), &["skill", "trash", "restore", "demo"]);

    assert!(
        !restore_output.status.success(),
        "restore unexpectedly succeeded"
    );
    assert_eq!(restore_env["ok"], Value::Bool(false));
    assert_eq!(
        restore_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    let live = fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read skill");
    assert!(live.contains("replacement"));
}

#[test]
fn skill_trash_purge_removes_one_trash_entry() {
    let root = TestDir::new("skill-trash-purge");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");

    let (trash_output, trash_env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");
    let trash_id = trash_env["data"]["trash_id"].as_str().expect("trash id");

    let (purge_output, purge_env) = run_loom(root.path(), &["skill", "trash", "purge", trash_id]);
    assert_success(&purge_output, "trash purge");
    assert_eq!(purge_env["ok"], Value::Bool(true));
    assert!(!root.path().join("trash").join(trash_id).exists());
    assert!(operations_log(root.path()).contains(r#""intent":"skill.trash.purge""#));
}
