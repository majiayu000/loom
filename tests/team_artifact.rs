mod common;
#[path = "../src/sha256.rs"]
mod sha256;
use common::{TestDir, run_loom, run_loom_with_env};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn archive(root: &Path, version: &str) -> (String, String) {
    let body = format!(
        "---\nname: demo\ndescription: Use when reviewing test changes.\n---\n# Demo\n\nReview changes carefully. {version}\n"
    );
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "SKILL.md", body.as_bytes())
        .unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gzip, &tar.into_inner().unwrap()).unwrap();
    let bytes = gzip.finish().unwrap();
    let mut hash = sha256::Sha256::new();
    hash.update(&bytes);
    let archive = root.join(format!("{version}.tar.gz"));
    fs::write(&archive, bytes).unwrap();
    let manifest = root.join(format!("{version}.json"));
    fs::write(&manifest, serde_json::to_vec(&json!({"service_origin":"https://team.example.test","team_id":"t1","skill_id":"s1","version_id":version,"sha256":sha256::to_hex(&hash.finalize()),"requested_ref":version})).unwrap()).unwrap();
    (
        archive.to_str().unwrap().into(),
        manifest.to_str().unwrap().into(),
    )
}
fn init(root: &Path) {
    let (out, env) = run_loom(root, &["workspace", "init"]);
    assert!(out.status.success(), "{env}");
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--exit-code",
            "HEAD",
            "--",
            "state/registry/ops/checkpoint.json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "init must commit its checkpoint");
}
fn plan(root: &Path, download: &Path, version: &str) -> Value {
    let (archive, manifest) = archive(download, version);
    let (out, env) = run_loom(
        root,
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(out.status.success(), "plan: {env}");
    assert_eq!(env["data"]["safe_to_apply"], true, "{env}");
    env["data"].clone()
}
fn apply(root: &Path, plan: &Value, faults: &[(&str, &str)]) -> (std::process::Output, Value) {
    run_loom_with_env(
        root,
        faults,
        &[
            "apply",
            plan["plan_id"].as_str().unwrap(),
            "--plan-digest",
            plan["plan_digest"].as_str().unwrap(),
            "--idempotency-key",
            "team-test",
        ],
    )
}
#[test]
fn fresh_install_and_same_source_update() {
    let root = TestDir::new("team-root");
    let download = TestDir::new("team-download");
    init(root.path());
    let first = plan(root.path(), download.path(), "v1");
    let (out, env) = apply(root.path(), &first, &[]);
    assert!(out.status.success(), "first apply {env}");
    let lock: Value =
        serde_json::from_slice(&fs::read(root.path().join("loom.lock")).unwrap()).unwrap();
    assert_eq!(lock["skills"]["demo"]["provider"], "team");
    assert_eq!(lock["skills"]["demo"]["team"]["version_id"], "v1");
    assert!(lock["skills"]["demo"]["commit"].is_null());
    let (out, inspected) = run_loom(root.path(), &["skill", "inspect", "demo"]);
    assert!(out.status.success(), "{inspected}");
    assert_eq!(inspected["data"]["provenance"]["team"]["version_id"], "v1");
    assert_eq!(
        inspected["data"]["provenance"]["team"]["service_origin"],
        "https://team.example.test"
    );
    let second = plan(root.path(), download.path(), "v2");
    let (out, env) = run_loom(
        root.path(),
        &[
            "apply",
            second["plan_id"].as_str().unwrap(),
            "--plan-digest",
            second["plan_digest"].as_str().unwrap(),
            "--idempotency-key",
            "team-update",
        ],
    );
    assert!(out.status.success(), "update {env}");
    assert!(
        fs::read_to_string(root.path().join("skills/demo/SKILL.md"))
            .unwrap()
            .contains("v2")
    );
}
#[test]
fn interrupted_team_metadata_recovers() {
    let root = TestDir::new("team-recovery");
    let download = TestDir::new("team-download");
    init(root.path());
    let plan = plan(root.path(), download.path(), "v1");
    let (out, env) = apply(
        root.path(),
        &plan,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_team_metadata",
        )],
    );
    assert!(!out.status.success(), "fault must interrupt {env}");
    let (out, env) = apply(root.path(), &plan, &[]);
    assert!(out.status.success(), "recovery {env}");
}
#[test]
fn refuses_hash_mismatch_without_registry_changes() {
    let root = TestDir::new("team-hash");
    let download = TestDir::new("team-download");
    init(root.path());
    let (archive, manifest) = archive(download.path(), "v1");
    fs::write(&archive, b"changed").unwrap();
    let (out, env) = run_loom(
        root.path(),
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(!out.status.success(), "{env}");
    assert!(!root.path().join("skills/demo").exists());
    assert!(!root.path().join("loom.lock").exists());
}
#[test]
fn refuses_different_team_and_local_drift() {
    let root = TestDir::new("team-conflict");
    let download = TestDir::new("team-download");
    init(root.path());
    let first = plan(root.path(), download.path(), "v1");
    let (out, env) = apply(root.path(), &first, &[]);
    assert!(out.status.success(), "{env}");
    let (archive, manifest) = archive(download.path(), "v2");
    let mut manifest_value: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    manifest_value["team_id"] = json!("different");
    fs::write(&manifest, serde_json::to_vec(&manifest_value).unwrap()).unwrap();
    let (out, env) = run_loom(
        root.path(),
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(!out.status.success(), "{env}");
    manifest_value["team_id"] = json!("t1");
    fs::write(&manifest, serde_json::to_vec(&manifest_value).unwrap()).unwrap();
    fs::write(root.path().join("skills/demo/local.txt"), "my changes").unwrap();
    let (out, env) = run_loom(
        root.path(),
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(!out.status.success(), "{env}");
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/local.txt")).unwrap(),
        "my changes"
    );
}
#[test]
fn updates_all_copy_projections_and_preserves_projection_edits() {
    use common::actions::{binding_add, skill_project, target_add};
    let root = TestDir::new("team-projections");
    let download = TestDir::new("team-download");
    init(root.path());
    let first = plan(root.path(), download.path(), "v1");
    let (out, env) = apply(root.path(), &first, &[]);
    assert!(out.status.success(), "{env}");
    let targets = [TestDir::new("team-target-a"), TestDir::new("team-target-b")];
    for target in &targets {
        let (out, env) = target_add(root.path(), "claude", target.path(), "managed");
        assert!(out.status.success(), "{env}");
        let target_id = env["data"]["target"]["target_id"].as_str().unwrap();
        let (out, env) = binding_add(
            root.path(),
            "claude",
            "default",
            "exact-path",
            target.path().to_str().unwrap(),
            target_id,
        );
        assert!(out.status.success(), "{env}");
        let binding_id = env["data"]["binding"]["binding_id"].as_str().unwrap();
        let (out, env) = skill_project(root.path(), "demo", binding_id, Some("copy"));
        assert!(out.status.success(), "{env}");
    }
    let second = plan(root.path(), download.path(), "v2");
    assert_eq!(second["effects"].as_array().unwrap().len(), 2);
    let (out, env) = run_loom_with_env(
        root.path(),
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_projection_swap",
        )],
        &[
            "apply",
            second["plan_id"].as_str().unwrap(),
            "--plan-digest",
            second["plan_digest"].as_str().unwrap(),
            "--idempotency-key",
            "multi-update",
        ],
    );
    assert!(
        !out.status.success(),
        "interrupted projection update: {env}"
    );
    let (out, env) = run_loom(
        root.path(),
        &[
            "apply",
            second["plan_id"].as_str().unwrap(),
            "--plan-digest",
            second["plan_digest"].as_str().unwrap(),
            "--idempotency-key",
            "multi-update",
        ],
    );
    assert!(out.status.success(), "{env}");
    for target in &targets {
        assert!(
            fs::read_to_string(target.path().join("demo/SKILL.md"))
                .unwrap()
                .contains("v2")
        );
    }
    fs::write(targets[0].path().join("demo/local.txt"), "keep my edits").unwrap();
    let (archive, manifest) = archive(download.path(), "v3");
    let (out, env) = run_loom(
        root.path(),
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(out.status.success(), "{env}");
    assert_eq!(env["data"]["safe_to_apply"], false, "{env}");
    let (out, env) = run_loom(
        root.path(),
        &[
            "apply",
            env["data"]["plan_id"].as_str().unwrap(),
            "--plan-digest",
            env["data"]["plan_digest"].as_str().unwrap(),
            "--idempotency-key",
            "blocked-update",
        ],
    );
    assert!(!out.status.success(), "{env}");
    assert_eq!(
        fs::read_to_string(targets[0].path().join("demo/local.txt")).unwrap(),
        "keep my edits"
    );
}
#[test]
fn crash_recovery_covers_source_and_partial_metadata_boundaries() {
    for fault in [
        "convergence_interrupt_after_source_replacement",
        "convergence_interrupt_after_team_sources",
        "convergence_interrupt_after_source_cas",
        "convergence_interrupt_after_source_commit",
    ] {
        let root = TestDir::new("team-crash");
        let download = TestDir::new("team-download");
        init(root.path());
        let plan = plan(root.path(), download.path(), "v1");
        let (out, env) = apply(root.path(), &plan, &[("LOOM_FAULT_INJECT", fault)]);
        assert!(!out.status.success(), "{fault}: {env}");
        let (out, env) = apply(root.path(), &plan, &[]);
        assert!(out.status.success(), "{fault}: {env}");
        let (out, env) = run_loom(root.path(), &["skill", "provenance", "verify", "demo"]);
        assert!(out.status.success(), "{env}");
        assert_eq!(env["data"]["matches"], true, "{fault}: {env}");
    }
}
#[test]
fn changed_source_after_preview_is_preserved() {
    let root = TestDir::new("team-preview-drift");
    let download = TestDir::new("team-download");
    init(root.path());
    let first = plan(root.path(), download.path(), "v1");
    let (out, env) = apply(root.path(), &first, &[]);
    assert!(out.status.success(), "{env}");
    let second = plan(root.path(), download.path(), "v2");
    fs::write(root.path().join("skills/demo/local.txt"), "after preview").unwrap();
    let (out, env) = run_loom(
        root.path(),
        &[
            "apply",
            second["plan_id"].as_str().unwrap(),
            "--plan-digest",
            second["plan_digest"].as_str().unwrap(),
            "--idempotency-key",
            "drift-update",
        ],
    );
    assert!(!out.status.success(), "{env}");
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/local.txt")).unwrap(),
        "after preview"
    );
    let lock: Value =
        serde_json::from_slice(&fs::read(root.path().join("loom.lock")).unwrap()).unwrap();
    assert_eq!(lock["skills"]["demo"]["team"]["version_id"], "v1");
}
#[test]
fn workspace_init_keeps_unrelated_staged_files_out_of_its_commits() {
    let root = TestDir::new("team-init-staged");
    for args in [
        vec!["init"],
        vec!["config", "user.name", "Loom Test"],
        vec!["config", "user.email", "test@example.test"],
        vec!["commit", "--allow-empty", "-m", "fixture"],
    ] {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    fs::write(root.path().join("notes.txt"), "unrelated user edits").unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(["add", "notes.txt"])
            .status()
            .unwrap()
            .success()
    );
    init(root.path());
    let committed = std::process::Command::new("git")
        .arg("-C")
        .arg(root.path())
        .args(["cat-file", "-e", "HEAD:notes.txt"])
        .output()
        .unwrap();
    assert!(!committed.status.success());
    let staged = std::process::Command::new("git")
        .arg("-C")
        .arg(root.path())
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(staged.stdout).unwrap().trim(),
        "notes.txt"
    );
}
#[test]
fn organization_policy_still_blocks_team_import() {
    let root = TestDir::new("team-org-policy");
    let download = TestDir::new("team-download");
    init(root.path());
    let (out, env) = run_loom(
        root.path(),
        &[
            "policy",
            "org",
            "init",
            "--bootstrap-admin",
            "test-team-admin",
        ],
    );
    assert!(out.status.success(), "{env}");
    let (archive, manifest) = archive(download.path(), "v1");
    let (out, env) = run_loom_with_env(
        root.path(),
        &[("USER", "unprivileged-user")],
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(!out.status.success(), "{env}");
    assert_eq!(env["error"]["code"], "POLICY_BLOCKED");
    assert!(!root.path().join("skills/demo").exists());
    let candidates = fs::read_dir(root.path().join("state/transactions"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("team-input-")
        })
        .count();
    assert_eq!(
        candidates, 0,
        "failed plan must clean its unpublished candidate"
    );
}
#[test]
fn provenance_refresh_cannot_erase_team_local_edit_protection() {
    let root = TestDir::new("team-refresh-drift");
    let download = TestDir::new("team-download");
    init(root.path());
    let first = plan(root.path(), download.path(), "v1");
    let (out, env) = apply(root.path(), &first, &[]);
    assert!(out.status.success(), "{env}");
    fs::write(
        root.path().join("skills/demo/local.txt"),
        "saved custom content",
    )
    .unwrap();
    let (out, env) = run_loom(root.path(), &["skill", "provenance", "refresh", "demo"]);
    assert!(out.status.success(), "{env}");
    for args in [
        vec!["add", "skills/demo/local.txt"],
        vec![
            "commit",
            "-m",
            "save local edits",
            "--",
            "skills/demo/local.txt",
        ],
    ] {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let (archive, manifest) = archive(download.path(), "v2");
    let (out, env) = run_loom(
        root.path(),
        &[
            "plan",
            "team-install",
            "demo",
            "--archive",
            &archive,
            "--manifest",
            &manifest,
        ],
    );
    assert!(!out.status.success(), "{env}");
    assert_eq!(
        fs::read_to_string(root.path().join("skills/demo/local.txt")).unwrap(),
        "saved custom content"
    );
}
