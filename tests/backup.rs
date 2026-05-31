mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{TestDir, run_loom, write_file};
use serde_json::Value;

#[test]
fn backup_export_inspect_restore_round_trips_registry_snapshot() {
    let root = TestDir::new("backup-round-trip");
    let (init_output, init_env) = run_loom(root.path(), &["workspace", "init"]);
    assert!(
        init_output.status.success(),
        "workspace init failed: stdout={} stderr={}",
        String::from_utf8_lossy(&init_output.stdout),
        String::from_utf8_lossy(&init_output.stderr)
    );
    assert_eq!(init_env["ok"], Value::Bool(true));

    write_file(
        &root.path().join("skills/demo-skill/SKILL.md"),
        "# Demo Skill\n\nBacked up while uncommitted.\n",
    );
    fs::create_dir_all(root.path().join("trash")).expect("create trash");
    write_file(&root.path().join("trash/old-skill/NOTE.md"), "removed\n");

    let head_before = git_text(root.path(), &["rev-parse", "HEAD"]);
    let operations_before =
        fs::read_to_string(root.path().join("state/registry/ops/operations.jsonl"))
            .expect("read operations before export");

    let (export_output, export_env) = run_loom(root.path(), &["backup", "export"]);
    assert!(
        export_output.status.success(),
        "backup export failed: stdout={} stderr={}",
        String::from_utf8_lossy(&export_output.stdout),
        String::from_utf8_lossy(&export_output.stderr)
    );
    assert_eq!(export_env["ok"], Value::Bool(true));
    let artifact = PathBuf::from(
        export_env["data"]["artifact"]
            .as_str()
            .expect("artifact path"),
    );
    assert!(
        artifact.is_file(),
        "artifact should exist: {}",
        artifact.display()
    );

    assert_eq!(git_text(root.path(), &["rev-parse", "HEAD"]), head_before);
    let operations_after =
        fs::read_to_string(root.path().join("state/registry/ops/operations.jsonl"))
            .expect("read operations after export");
    assert_eq!(
        operations_after, operations_before,
        "export must not mutate registry operations"
    );
    assert!(
        fs::read_to_string(root.path().join(".gitignore"))
            .expect("read .gitignore")
            .lines()
            .any(|line| line == "backups/"),
        "workspace .gitignore must ignore default backup artifacts"
    );
    assert_eq!(
        git_text(root.path(), &["status", "--short", "--", "backups"]),
        "",
        "default backup artifact must be ignored by Git"
    );

    let artifact_arg = artifact.to_string_lossy().into_owned();
    let (inspect_output, inspect_env) =
        run_loom(root.path(), &["backup", "inspect", &artifact_arg]);
    assert!(
        inspect_output.status.success(),
        "backup inspect failed: stdout={} stderr={}",
        String::from_utf8_lossy(&inspect_output.stdout),
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    assert_eq!(inspect_env["data"]["valid"], Value::Bool(true));
    assert_eq!(inspect_env["data"]["bundle_verified"], Value::Bool(true));
    assert_eq!(
        inspect_env["data"]["manifest"]["head"],
        Value::String(head_before.clone())
    );
    assert_eq!(
        inspect_env["data"]["manifest"]["counts"]["skills"],
        Value::from(1)
    );
    assert_eq!(
        inspect_env["data"]["manifest"]["counts"]["trash_entries"],
        Value::from(1)
    );

    let restored = TestDir::new("backup-restored");
    let restored_arg = restored.path().to_string_lossy().into_owned();
    let (restore_output, restore_env) = run_loom(
        root.path(),
        &["backup", "restore", &artifact_arg, "--root", &restored_arg],
    );
    assert!(
        restore_output.status.success(),
        "backup restore failed: stdout={} stderr={}",
        String::from_utf8_lossy(&restore_output.stdout),
        String::from_utf8_lossy(&restore_output.stderr)
    );
    assert_eq!(restore_env["data"]["restored"], Value::Bool(true));
    assert_eq!(
        restore_env["data"]["source_head"],
        Value::String(head_before)
    );
    assert!(restored.path().join(".git").is_dir());
    assert!(restored.path().join("state/registry").is_dir());
    assert_eq!(
        fs::read_to_string(restored.path().join("skills/demo-skill/SKILL.md"))
            .expect("read restored skill"),
        "# Demo Skill\n\nBacked up while uncommitted.\n"
    );
    assert!(restored.path().join("trash/old-skill/NOTE.md").is_file());

    let (status_output, status_env) = run_loom(restored.path(), &["workspace", "status"]);
    assert!(
        status_output.status.success(),
        "restored registry status failed: stdout={} stderr={}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    assert_eq!(status_env["ok"], Value::Bool(true));
}

#[test]
fn backup_restore_refuses_non_empty_destination() {
    let root = TestDir::new("backup-nonempty-source");
    let (init_output, _) = run_loom(root.path(), &["workspace", "init"]);
    assert!(init_output.status.success());
    let (export_output, export_env) = run_loom(root.path(), &["backup", "export"]);
    assert!(export_output.status.success());
    let artifact = export_env["data"]["artifact"].as_str().expect("artifact");

    let dst = TestDir::new("backup-nonempty-dst");
    write_file(&dst.path().join("keep.txt"), "do not overwrite\n");
    let dst_arg = dst.path().to_string_lossy().into_owned();
    let (restore_output, restore_env) = run_loom(
        root.path(),
        &["backup", "restore", artifact, "--root", &dst_arg],
    );

    assert!(!restore_output.status.success());
    assert_eq!(
        restore_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    assert_eq!(
        fs::read_to_string(dst.path().join("keep.txt")).expect("read keep"),
        "do not overwrite\n"
    );
}

#[test]
fn backup_inspect_rejects_malformed_artifact() {
    let root = TestDir::new("backup-malformed");
    let bad = root.path().join("bad.tar");
    fs::write(&bad, b"not a tar archive").expect("write malformed artifact");
    let bad_arg = bad.to_string_lossy().into_owned();

    let (inspect_output, inspect_env) = run_loom(root.path(), &["backup", "inspect", &bad_arg]);
    assert!(!inspect_output.status.success());
    assert_eq!(inspect_env["ok"], Value::Bool(false));
}

fn git_text(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("user.name=Loom Test")
        .arg("-c")
        .arg("user.email=loom@example.invalid")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stdout={} stderr={}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}
