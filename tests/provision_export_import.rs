mod common;

use std::fs;
use std::process::Command;

use common::{TestDir, run_loom, write_file, write_minimal_registry_state, write_skill};
use serde_json::json;

fn setup_provision_export_registry() -> (TestDir, String) {
    let root = TestDir::new("provision-export-root");
    write_minimal_registry_state(root.path(), 1);
    write_skill(
        root.path(),
        "model-onboarding",
        "---\nname: model-onboarding\ndescription: Demo.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/model-onboarding/loom.skill.toml"),
        "requires_env = [\"MODEL_TOKEN\"]\n",
    );
    Command::new("git")
        .arg("init")
        .arg(root.path())
        .output()
        .expect("git init");
    Command::new("git")
        .current_dir(root.path())
        .args([
            "remote",
            "add",
            "origin",
            "https://token@example.com/org/registry.git?token=secret",
        ])
        .output()
        .expect("git remote add");
    (root, "/tmp/project-a".to_string())
}

fn write_plan(root: &TestDir, workspace: &str, plan_path: &str) {
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--agent",
            "claude",
            "--workspace",
            workspace,
            "--output-plan",
            plan_path,
        ],
    );
    assert!(output.status.success(), "provision plan failed: {env}");
}

#[test]
fn provision_shell_export_is_deterministic_and_redacted() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.sh");
    let second_export_path = root.path().join("provision-second.sh");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    let second_export_arg = second_export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "shell",
            "--output",
            &export_arg,
        ],
    );

    assert!(output.status.success(), "provision export failed: {env}");
    assert_eq!(env["cmd"], json!("provision.export"));
    assert_eq!(env["data"]["artifact_written"], json!(true));
    assert_eq!(env["data"]["target_writes_performed"], json!(false));
    assert_eq!(env["data"]["artifact_kind"], json!("shell"));
    assert_eq!(
        env["data"]["source_path"],
        json!(".devcontainer/loom-setup.sh")
    );

    let (second_output, second_env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "shell",
            "--output",
            &second_export_arg,
        ],
    );
    assert!(
        second_output.status.success(),
        "second provision export failed: {second_env}"
    );

    let artifact = fs::read_to_string(&export_path).expect("read shell artifact");
    let second_artifact = fs::read_to_string(&second_export_path).expect("read second artifact");
    assert_eq!(artifact, second_artifact);
    assert!(artifact.starts_with("# loom-provision-artifact-v1\n"));
    assert!(artifact.contains("# schema_version=provision-shell-artifact-v1\n"));
    assert!(artifact.contains("#!/usr/bin/env bash\n"));
    assert!(artifact.contains("set -euo pipefail"));
    assert!(artifact.contains("https://example.com/org/registry.git"));
    assert!(!artifact.contains("token@"));
    assert!(!artifact.contains("token=secret"));
    assert!(!artifact.contains("MODEL_TOKEN="));
}

#[test]
fn provision_import_dry_run_inspects_shell_artifact_without_writes() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.sh");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);
    let (export_output, export_env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "shell",
            "--output",
            &export_arg,
        ],
    );
    assert!(
        export_output.status.success(),
        "provision export failed: {export_env}"
    );

    let (output, env) = run_loom(
        root.path(),
        &["provision", "import", &export_arg, "--dry-run"],
    );

    assert!(output.status.success(), "provision import failed: {env}");
    assert_eq!(env["cmd"], json!("provision.import"));
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(env["data"]["artifact_kind"], json!("shell"));
    assert_eq!(env["data"]["target_kind"], json!("devcontainer"));
    assert_eq!(env["data"]["target_writes_performed"], json!(false));
    assert_eq!(
        env["data"]["planned_files"][0]["path"],
        json!(".devcontainer/loom-setup.sh")
    );
    assert_eq!(
        env["data"]["planned_files"][0]["action"],
        json!("review_only")
    );
    assert!(!root.path().join(".devcontainer").exists());
}

#[test]
fn provision_import_rejects_tampered_shell_artifact() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.sh");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);
    let (export_output, export_env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "shell",
            "--output",
            &export_arg,
        ],
    );
    assert!(
        export_output.status.success(),
        "provision export failed: {export_env}"
    );

    let tampered = fs::read_to_string(&export_path)
        .expect("read artifact")
        .replace("workspace status", "workspace doctor");
    write_file(&export_path, &tampered);
    let (output, env) = run_loom(
        root.path(),
        &["provision", "import", &export_arg, "--dry-run"],
    );

    assert!(!output.status.success(), "tampered import should fail");
    assert_eq!(env["cmd"], json!("provision.import"));
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(env["error"]["message"].as_str().unwrap().contains("digest"));
}
