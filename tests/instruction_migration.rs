mod common;
use common::{TestDir, run_loom, write_file, write_skill};
use serde_json::Value;
use std::fs;

fn setup(body: &str) -> (TestDir, TestDir, String) {
    let root = TestDir::new("migration-registry");
    let workspace = TestDir::new("migration-workspace");
    write_file(&workspace.path().join("AGENTS.md"), body);
    let (out, value) = run_loom(
        root.path(),
        &[
            "instruction",
            "scan",
            "--workspace",
            workspace.path().to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{value}");
    let id = value["data"]["surfaces"][0]["instruction_id"]
        .as_str()
        .unwrap()
        .to_string();
    (root, workspace, id)
}
fn plan(
    root: &TestDir,
    workspace: &TestDir,
    id: &str,
    target: &str,
) -> (std::process::Output, Value) {
    run_loom(
        root.path(),
        &[
            "instruction",
            "migrate-plan",
            id,
            "--workspace",
            workspace.path().to_str().unwrap(),
            "--to",
            target,
            "--name",
            "extracted",
        ],
    )
}
#[test]
fn reviewed_instruction_patch_applies_and_replays_without_modifying_source() {
    let body = "# Project guide\n\nRun focused tests before completing a change.\n";
    let (root, workspace, id) = setup(body);
    let (out, value) = plan(&root, &workspace, &id, "skill");
    assert!(out.status.success(), "{value}");
    assert_eq!(value["data"]["patch"]["artifact_written"], true);
    assert_eq!(value["data"]["patch"]["provider"], "local");
    assert!(!root.path().join("skills/extracted/SKILL.md").exists());
    let patch = value["data"]["patch"]["patch_id"].as_str().unwrap();
    let args = [
        "skill",
        "author",
        "apply-patch",
        patch,
        "--idempotency-key",
        "migration-1",
    ];
    let (out, value) = run_loom(root.path(), &args);
    assert!(out.status.success(), "{value}");
    let skill = fs::read_to_string(root.path().join("skills/extracted/SKILL.md")).unwrap();
    assert!(skill.contains(body));
    assert_eq!(
        fs::read_to_string(workspace.path().join("AGENTS.md")).unwrap(),
        body
    );
    let (out, value) = run_loom(root.path(), &args);
    assert!(out.status.success(), "{value}");
    assert_eq!(
        fs::read_to_string(root.path().join("skills/extracted/SKILL.md")).unwrap(),
        skill
    );
    let (out, value) = plan(&root, &workspace, &id, "skill");
    assert!(
        !out.status.success(),
        "must not overwrite an existing skill: {value}"
    );
}
#[test]
fn reference_extraction_preserves_entrypoint_and_requires_existing_skill() {
    let (root, workspace, id) = setup("# Background\nUseful reference content.\n");
    let (out, value) = plan(&root, &workspace, &id, "reference");
    assert!(!out.status.success(), "{value}");
    assert_eq!(value["error"]["code"], "SKILL_NOT_FOUND");
    let source = "---\nname: extracted\ndescription: Use when reviewing project guidance.\n---\n# Workflow\nRead references when needed.\n";
    write_skill(root.path(), "extracted", source);
    let (out, value) = plan(&root, &workspace, &id, "reference");
    assert!(out.status.success(), "{value}");
    let patch = value["data"]["patch"]["patch_id"].as_str().unwrap();
    let (out, value) = run_loom(
        root.path(),
        &[
            "skill",
            "author",
            "apply-patch",
            patch,
            "--idempotency-key",
            "reference-1",
        ],
    );
    assert!(out.status.success(), "{value}");
    assert!(
        fs::read_to_string(root.path().join("skills/extracted/references/agents.md"))
            .unwrap()
            .contains("Useful reference content.")
    );
    assert_eq!(
        fs::read_to_string(root.path().join("skills/extracted/SKILL.md")).unwrap(),
        source
    );
}
#[test]
fn migration_rejects_sensitive_content_instead_of_copying_it() {
    let (root, workspace, id) = setup("# Guide\napi_key=sk-test-sensitive-example-value\n");
    let (out, value) = plan(&root, &workspace, &id, "skill");
    assert!(!out.status.success(), "{value}");
    assert_eq!(value["error"]["code"], "POLICY_BLOCKED");
    assert!(!root.path().join("state/patches").exists());
}
