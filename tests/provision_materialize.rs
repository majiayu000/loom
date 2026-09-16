mod common;
use common::{TestDir, run_loom, write_skill};
use std::{fs, process::Command};

fn fixture(target: &str) -> (TestDir, TestDir, String) {
    let root = TestDir::new("provision-bundle");
    let workspace = TestDir::new("provision-destination");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing portable workspace import.\n---\n# Demo\nPortable body.\n",
    );
    let (out, value) = run_loom(
        root.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--scope",
            "project",
            "--workspace",
            workspace.path().to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{value}");
    for args in [
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    ] {
        let out = Command::new("git")
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let (out, value) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            target,
            "--workspace",
            workspace.path().to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{value}");
    let plan = value["data"]["plan"]["plan_id"]
        .as_str()
        .unwrap()
        .to_string();
    (root, workspace, plan)
}
fn export(root: &TestDir, plan: &str, format: &str, name: &str) -> String {
    let path = root.path().join(name).display().to_string();
    let (out, value) = run_loom(
        root.path(),
        &[
            "provision",
            "export",
            plan,
            "--format",
            format,
            "--output",
            &path,
        ],
    );
    assert!(out.status.success(), "{value}");
    path
}
#[test]
fn imported_tar_materializes_generated_registry_and_active_view_files_without_execution() {
    let (root, _workspace, plan) = fixture("devcontainer");
    let artifact = export(&root, &plan, "tar", "bundle.tar");
    let output = root.path().join("imported");
    let args = [
        "provision",
        "import",
        &artifact,
        "--output",
        output.to_str().unwrap(),
    ];
    let mut preview = args.to_vec();
    preview.push("--dry-run");
    let (out, value) = run_loom(root.path(), &preview);
    assert!(out.status.success(), "{value}");
    assert!(!output.exists());
    let (out, value) = run_loom(root.path(), &args);
    assert!(out.status.success(), "{value}");
    assert_eq!(value["data"]["scripts_executed"], false);
    assert!(output.join(".devcontainer/devcontainer.json").is_file());
    assert_eq!(
        fs::read(output.join(".agents/skills/demo/SKILL.md")).unwrap(),
        fs::read(output.join("registry/skills/demo/SKILL.md")).unwrap()
    );
    assert!(!output.join(".git").exists());
    let before = fs::read(output.join(".agents/skills/demo/SKILL.md")).unwrap();
    let (out, value) = run_loom(root.path(), &args);
    assert!(!out.status.success(), "{value}");
    assert_eq!(
        fs::read(output.join(".agents/skills/demo/SKILL.md")).unwrap(),
        before
    );
}
#[test]
fn codespaces_supports_directory_export_and_approved_local_apply() {
    let (root, workspace, plan) = fixture("codespaces");
    let dir = export(&root, &plan, "devcontainer", "exported-config");
    assert!(
        std::path::Path::new(&dir)
            .join(".devcontainer/devcontainer.json")
            .exists()
    );
    let args = [
        "provision",
        "apply",
        &plan,
        "--idempotency-key",
        "codespaces",
    ];
    let (out, value) = run_loom(root.path(), &args);
    assert!(!out.status.success(), "{value}");
    let mut approved = args.to_vec();
    approved.extend(["--approve", "approval:provision-apply"]);
    let (out, value) = run_loom(root.path(), &approved);
    assert!(out.status.success(), "{value}");
    assert_eq!(
        fs::read(workspace.path().join(".devcontainer/devcontainer.json")).unwrap(),
        fs::read(std::path::Path::new(&dir).join(".devcontainer/devcontainer.json")).unwrap()
    );
}
#[test]
fn shell_import_materializes_only_reviewed_script_and_rejects_escape() {
    let (root, _workspace, plan) = fixture("remote");
    let artifact = export(&root, &plan, "shell", "bundle.sh");
    let output = root.path().join("shell-import");
    let (out, value) = run_loom(
        root.path(),
        &[
            "provision",
            "import",
            &artifact,
            "--output",
            output.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{value}");
    assert!(output.join(".loom/loom-setup.sh").is_file());
    assert!(!output.join(".devcontainer").exists());
    let raw = fs::read_to_string(&artifact).unwrap().replace(
        "# source_path=.loom/loom-setup.sh",
        "# source_path=../escape.sh",
    );
    fs::write(&artifact, raw).unwrap();
    let bad = root.path().join("bad-import");
    let (out, value) = run_loom(
        root.path(),
        &[
            "provision",
            "import",
            &artifact,
            "--output",
            bad.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success(), "{value}");
    assert!(!bad.exists());
    assert!(!root.path().join("escape.sh").exists());
}
