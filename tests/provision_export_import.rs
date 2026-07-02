mod common;

use std::fs;
use std::io::Read;
use std::process::Command;

use common::{TestDir, run_loom, write_file, write_minimal_registry_state, write_skill};
use serde_json::{Value, json};
use tar::Archive;

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
    commit_all(&root, "seed provision registry");
    (root, "/tmp/project-a".to_string())
}

fn commit_all(root: &TestDir, message: &str) {
    let add = Command::new("git")
        .current_dir(root.path())
        .args(["add", "."])
        .output()
        .expect("git add");
    assert!(
        add.status.success(),
        "git add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let commit = Command::new("git")
        .current_dir(root.path())
        .args([
            "-c",
            "user.name=Loom Test",
            "-c",
            "user.email=loom-test@example.com",
            "commit",
            "-m",
            message,
        ])
        .output()
        .expect("git commit");
    assert!(
        commit.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
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

fn tar_entries(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let file = fs::File::open(path).expect("open tar artifact");
    let mut archive = Archive::new(file);
    let mut entries = Vec::new();
    for entry in archive.entries().expect("tar entries") {
        let mut entry = entry.expect("tar entry");
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().expect("entry path").display().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        entries.push((path, bytes));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
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
fn provision_tar_export_rejects_secret_looking_source_content() {
    let (root, workspace) = setup_provision_export_registry();
    write_file(
        &root.path().join("skills/model-onboarding/notes.txt"),
        "token=local-secret\n",
    );
    commit_all(&root, "add secret-looking source");
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(!output.status.success(), "secret source export should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("secret-looking")
    );
    assert!(!env.to_string().contains("local-secret"));
}

#[test]
fn provision_tar_export_rejects_secret_named_source_files() {
    let (root, workspace) = setup_provision_export_registry();
    write_file(
        &root.path().join("skills/model-onboarding/credentials.json"),
        "{}\n",
    );
    commit_all(&root, "add credential-named source");
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(
        !output.status.success(),
        "credential source export should fail"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("user-specific config")
    );
}

#[test]
fn provision_tar_export_rejects_stale_reviewed_registry_head() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);
    write_file(
        &root.path().join("skills/model-onboarding/SKILL.md"),
        "---\nname: model-onboarding\ndescription: Updated.\n---\n# Updated\n",
    );
    commit_all(&root, "change source after reviewed plan");

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(!output.status.success(), "stale source export should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("registry head is stale")
    );
}

#[test]
fn provision_tar_export_rejects_invalid_skill_ids_before_path_join() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);
    let mut plan: Value =
        serde_json::from_str(&fs::read_to_string(&plan_path).expect("read plan artifact"))
            .expect("parse plan artifact");
    plan["active_views"][0]["skills"][0] = json!("../state/registry");
    write_file(
        &plan_path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&plan).expect("serialize plan")
        ),
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(!output.status.success(), "invalid skill id should fail");
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unsupported characters")
    );
}

#[test]
fn provision_tar_export_rejects_output_inside_packaged_skill_source() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root
        .path()
        .join("skills/model-onboarding/previous-provision.tar");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(!output.status.success(), "nested output export should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("inside packaged skill source")
    );
}

#[cfg(unix)]
#[test]
fn provision_tar_export_rejects_hardlinked_skill_sources() {
    let (root, workspace) = setup_provision_export_registry();
    let outside = root.path().join("outside-source.txt");
    let linked = root.path().join("skills/model-onboarding/hardlinked.txt");
    write_file(&outside, "external source\n");
    fs::hard_link(&outside, &linked).expect("create hardlink");
    commit_all(&root, "add hardlinked source");
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
    let plan_arg = plan_path.to_string_lossy().to_string();
    let export_arg = export_path.to_string_lossy().to_string();
    write_plan(&root, &workspace, &plan_arg);

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(
        !output.status.success(),
        "hardlinked source export should fail"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("hardlink")
    );
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

#[test]
fn provision_tar_export_is_deterministic_and_redacted() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
    let second_export_path = root.path().join("provision-second.tar");
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
            "tar",
            "--output",
            &export_arg,
        ],
    );

    assert!(
        output.status.success(),
        "provision tar export failed: {env}"
    );
    assert_eq!(env["cmd"], json!("provision.export"));
    assert_eq!(env["data"]["artifact_kind"], json!("tar"));
    assert_eq!(env["data"]["artifact_written"], json!(true));
    assert_eq!(env["data"]["target_writes_performed"], json!(false));
    assert_eq!(env["data"]["generated_file_count"], json!(2));
    assert!(env["data"]["registry_file_count"].as_u64().unwrap() >= 2);
    assert!(env["data"]["active_view_file_count"].as_u64().unwrap() >= 2);

    let (second_output, second_env) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            &plan_arg,
            "--format",
            "tar",
            "--output",
            &second_export_arg,
        ],
    );
    assert!(
        second_output.status.success(),
        "second tar export failed: {second_env}"
    );
    assert_eq!(
        fs::read(&export_path).expect("read tar"),
        fs::read(&second_export_path).expect("read second tar")
    );

    let entries = tar_entries(&export_path);
    let names = entries
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    let root_prefix = names[0].split('/').next().unwrap().to_string();
    assert!(names.contains(&format!("{root_prefix}/manifest.json").as_str()));
    assert!(names.contains(&format!("{root_prefix}/checksums.txt").as_str()));
    assert!(names.contains(&format!("{root_prefix}/plan.json").as_str()));
    assert!(names.contains(&format!("{root_prefix}/files/.devcontainer/loom-setup.sh").as_str()));
    assert!(
        names
            .contains(&format!("{root_prefix}/registry/skills/model-onboarding/SKILL.md").as_str())
    );
    assert!(names.contains(
        &format!("{root_prefix}/active-views/claude-0/model-onboarding/SKILL.md").as_str()
    ));

    let joined = entries
        .iter()
        .flat_map(|(_, bytes)| bytes.iter().copied())
        .collect::<Vec<_>>();
    let text = String::from_utf8_lossy(&joined);
    assert!(!text.contains("token@"));
    assert!(!text.contains("token=secret"));
    assert!(!text.contains("MODEL_TOKEN="));
}

#[test]
fn provision_import_dry_run_inspects_tar_artifact_without_writes() {
    let (root, workspace) = setup_provision_export_registry();
    let plan_path = root.path().join("provision-plan.json");
    let export_path = root.path().join("provision.tar");
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
            "tar",
            "--output",
            &export_arg,
        ],
    );
    assert!(
        export_output.status.success(),
        "provision tar export failed: {export_env}"
    );

    let (output, env) = run_loom(
        root.path(),
        &["provision", "import", &export_arg, "--dry-run"],
    );

    assert!(
        output.status.success(),
        "provision tar import failed: {env}"
    );
    assert_eq!(env["cmd"], json!("provision.import"));
    assert_eq!(env["data"]["artifact_kind"], json!("tar"));
    assert_eq!(env["data"]["checksums_verified"], json!(true));
    assert_eq!(env["data"]["target_writes_performed"], json!(false));
    assert_eq!(env["data"]["planned_files"].as_array().unwrap().len(), 2);
    assert!(
        env["data"]["planned_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == ".devcontainer/loom-setup.sh")
    );
    assert!(!root.path().join(".devcontainer").exists());
}
