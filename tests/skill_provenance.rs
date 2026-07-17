mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::actions::{binding_add, skill_project, target_add};
use common::{TestDir, operations_log, run_loom, run_loom_with_env, write_file};
use serde_json::{Value, json};

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read json")).expect("parse json")
}

fn first_outdated_row(env: &Value) -> &Value {
    &env["data"]["rows"].as_array().expect("rows array")[0]
}

fn init_git_skill_repo(repo: &Path) -> (String, String) {
    let git = |args: &[&str]| -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("-c")
            .arg("commit.gpgsign=false")
            .arg("-c")
            .arg("tag.gpgSign=false")
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
    };

    fs::create_dir_all(repo.join("skill")).expect("create skill dir");
    write_file(&repo.join("skill/SKILL.md"), "# demo\n\nversion one\n");
    git(&["init"]);
    git(&["config", "--local", "user.name", "Skill Source"]);
    git(&["config", "--local", "user.email", "source@example.com"]);
    git(&["branch", "-M", "main"]);
    git(&["add", "skill/SKILL.md"]);
    git(&["commit", "-m", "skill v1"]);
    let v1 = git(&["rev-parse", "HEAD"]);
    git(&["tag", "v1"]);

    write_file(&repo.join("skill/SKILL.md"), "# demo\n\nversion two\n");
    git(&["add", "skill/SKILL.md"]);
    git(&["commit", "-m", "skill v2"]);
    let v2 = git(&["rev-parse", "HEAD"]);
    (v1, v2)
}

#[test]
fn skill_provenance_outdated_reports_local_digest_status_and_review_plan() {
    let root = TestDir::new("skill-provenance-outdated-local");
    let catalog = TestDir::new("skill-provenance-outdated-local-catalog");
    let skill = catalog.path().join("skills/demo");
    write_file(&skill.join("SKILL.md"), "# Demo\n\nversion one\n");

    let locator = format!("local:{}//skills/demo", catalog.path().display());
    let (output, preview) = run_loom(root.path(), &["catalog", "preview", &locator]);
    assert!(output.status.success(), "preview should pass: {preview}");
    let digest_v1 = preview["data"]["preview"]["provenance"]["digest"]
        .as_str()
        .expect("preview digest")
        .to_string();
    let pinned_v1 = format!("{locator}@{digest_v1}");
    let (output, install) = run_loom(
        root.path(),
        &["skill", "install", &pinned_v1, "--name", "demo"],
    );
    assert!(output.status.success(), "install should pass: {install}");

    let (output, report) = run_loom(root.path(), &["skill", "provenance", "outdated", "demo"]);
    assert!(output.status.success(), "outdated should pass: {report}");
    let row = first_outdated_row(&report);
    assert_eq!(row["status"], json!("up_to_date"));
    assert_eq!(row["provider"], json!("local"));
    assert_eq!(row["current_ref"], json!(digest_v1));
    assert_eq!(row["current_digest"], json!(digest_v1));
    assert_eq!(row["candidate_ref"], json!(digest_v1));
    assert_eq!(row["candidate_digest"], json!(digest_v1));

    write_file(&skill.join("SKILL.md"), "# Demo\n\nversion two\n");
    let sources_before =
        fs::read_to_string(root.path().join("state/registry/sources.json")).expect("sources");
    let lock_before = fs::read_to_string(root.path().join("loom.lock")).expect("lock");
    let (output, plan) = run_loom(
        root.path(),
        &["skill", "provenance", "outdated", "demo", "--plan"],
    );
    assert!(output.status.success(), "outdated plan should pass: {plan}");
    let row = first_outdated_row(&plan);
    assert_eq!(row["status"], json!("outdated"));
    assert_eq!(row["candidate_trust"], json!("immutable"));
    assert_ne!(row["candidate_digest"], json!(digest_v1));
    assert_eq!(plan["data"]["re_pin_plan"]["mutates"], json!(false));
    assert_eq!(plan["data"]["re_pin_plan"]["apply_required"], json!(true));
    assert_eq!(
        plan["data"]["re_pin_plan"]["items"][0]["mutates"],
        json!(false)
    );
    assert_eq!(
        plan["data"]["re_pin_plan"]["items"][0]["candidate"]["digest"],
        row["candidate_digest"]
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/sources.json")).expect("sources after"),
        sources_before,
        "outdated --plan must not update sources.json"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("loom.lock")).expect("lock after"),
        lock_before,
        "outdated --plan must not update loom.lock"
    );
}

#[test]
fn skill_provenance_outdated_distinguishes_unreachable_unpinned_and_invalid_sources() {
    let root = TestDir::new("skill-provenance-outdated-states");
    let catalog = TestDir::new("skill-provenance-outdated-states-catalog");
    let skill = catalog.path().join("skills/demo");
    write_file(&skill.join("SKILL.md"), "# Demo\n\nversion one\n");

    let locator = format!("local:{}//skills/demo", catalog.path().display());
    let (output, preview) = run_loom(root.path(), &["catalog", "preview", &locator]);
    assert!(output.status.success(), "preview should pass: {preview}");
    let digest = preview["data"]["preview"]["provenance"]["digest"]
        .as_str()
        .expect("preview digest")
        .to_string();
    let pinned = format!("{locator}@{digest}");
    let (output, install) = run_loom(
        root.path(),
        &["skill", "install", &pinned, "--name", "demo"],
    );
    assert!(output.status.success(), "install should pass: {install}");

    let mut sources = read_json(&root.path().join("state/registry/sources.json"));
    sources["sources"][0]["source"]["requested_ref"] = json!("latest");
    write_file(
        &root.path().join("state/registry/sources.json"),
        &(serde_json::to_string_pretty(&sources).expect("serialize sources") + "\n"),
    );
    let (output, unpinned) = run_loom(root.path(), &["skill", "provenance", "outdated", "demo"]);
    assert!(
        output.status.success(),
        "unpinned source report should pass: {unpinned}"
    );
    let row = first_outdated_row(&unpinned);
    assert_eq!(row["status"], json!("unpinned_candidate"));
    assert_eq!(row["candidate_trust"], json!("advisory"));
    assert_eq!(row["candidate_digest"], json!(digest));

    sources["sources"][0]["source"]["requested_ref"] = json!(digest);
    write_file(
        &root.path().join("state/registry/sources.json"),
        &(serde_json::to_string_pretty(&sources).expect("serialize sources") + "\n"),
    );
    fs::remove_dir_all(&skill).expect("remove provider skill");
    let (output, unreachable) = run_loom(root.path(), &["skill", "provenance", "outdated", "demo"]);
    assert!(
        output.status.success(),
        "unreachable source report should pass: {unreachable}"
    );
    let row = first_outdated_row(&unreachable);
    assert_eq!(row["status"], json!("unreachable"));
    assert!(
        row["error"]
            .as_str()
            .expect("error")
            .contains("not a directory")
    );

    let local_source = TestDir::new("skill-provenance-outdated-local-path");
    write_file(
        &local_source.path().join("SKILL.md"),
        "# Local\n\nnot provider-backed\n",
    );
    let source_arg = local_source.path().to_str().expect("source path");
    let (output, add) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "plain-local"],
    );
    assert!(output.status.success(), "skill add should pass: {add}");
    let (output, invalid) = run_loom(
        root.path(),
        &["skill", "provenance", "outdated", "plain-local"],
    );
    assert!(
        output.status.success(),
        "invalid source report should pass: {invalid}"
    );
    assert_eq!(
        first_outdated_row(&invalid)["status"],
        json!("invalid_source")
    );
}

#[test]
fn skill_provenance_outdated_resolves_github_pinned_commit_candidate_digest() {
    let root = TestDir::new("skill-provenance-outdated-github");
    let source = TestDir::new("skill-provenance-outdated-github-source");
    let (v1, v2) = init_git_skill_repo(source.path());
    let source_arg = source.path().to_str().expect("source path");
    let (output, add) = run_loom(
        root.path(),
        &[
            "skill", "add", source_arg, "--name", "demo", "--ref", &v1, "--subdir", "skill",
        ],
    );
    assert!(output.status.success(), "skill add should pass: {add}");

    let mut sources = read_json(&root.path().join("state/registry/sources.json"));
    sources["sources"][0]["source"]["provider"] = json!("github");
    sources["sources"][0]["source"]["locator"] = json!(format!("github:local/demo//skill@{v1}"));
    sources["sources"][0]["source"]["repository"] = json!(source_arg);
    write_file(
        &root.path().join("state/registry/sources.json"),
        &(serde_json::to_string_pretty(&sources).expect("serialize sources") + "\n"),
    );

    let (output, report) = run_loom(root.path(), &["skill", "provenance", "outdated", "demo"]);
    assert!(output.status.success(), "outdated should pass: {report}");
    let row = first_outdated_row(&report);
    assert_eq!(row["provider"], json!("github"));
    assert_eq!(row["status"], json!("outdated"));
    assert_eq!(row["current_ref"], json!(v1));
    assert_eq!(row["candidate_ref"], json!(v2));
    assert_ne!(row["candidate_digest"], row["current_digest"]);
    assert_eq!(row["candidate_trust"], json!("immutable"));
}

#[test]
fn skill_add_records_local_path_provenance_and_lock() {
    let root = TestDir::new("skill-provenance-local");
    let source = TestDir::new("skill-provenance-source");
    write_file(
        &source.path().join("SKILL.md"),
        "# demo\n\nlocal path skill\n",
    );

    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(root.path(), &["skill", "add", source_arg, "--name", "demo"]);
    assert!(output.status.success(), "skill add should pass: {env}");

    let sources = read_json(&root.path().join("state/registry/sources.json"));
    assert_eq!(sources["schema_version"], json!(1));
    let record = &sources["sources"][0];
    assert_eq!(record["skill_id"], json!("demo"));
    assert_eq!(record["source"]["provider"], json!("local_path"));
    assert_eq!(
        record["source"]["path"],
        json!(
            fs::canonicalize(source.path())
                .expect("canonical source")
                .display()
                .to_string()
        )
    );
    assert_eq!(record["source"]["subdir"], json!(""));
    assert_eq!(
        record["importer_version"],
        json!(format!("loom/{}", env!("CARGO_PKG_VERSION")))
    );
    let digest = record["artifact"]["digest"]
        .as_str()
        .expect("record digest");
    assert!(
        digest.starts_with("sha256:"),
        "digest should be sha256: {digest}"
    );

    let lock = read_json(&root.path().join("loom.lock"));
    assert_eq!(lock["version"], json!(1));
    assert_eq!(lock["skills"]["demo"]["provider"], json!("local_path"));
    assert_eq!(lock["skills"]["demo"]["digest"], json!(digest));
    assert_eq!(lock["skills"]["demo"]["agents"], json!([]));
    assert_eq!(lock["skills"]["demo"]["scope"], json!("project"));

    let (output, inspect) = run_loom(root.path(), &["skill", "provenance", "inspect", "demo"]);
    assert!(output.status.success(), "inspect should pass: {inspect}");
    assert_eq!(
        inspect["data"]["provenance"]["artifact"]["digest"],
        json!(digest)
    );
    assert_eq!(inspect["data"]["lock"]["digest"], json!(digest));

    let (output, verify) = run_loom(root.path(), &["skill", "provenance", "verify", "demo"]);
    assert!(output.status.success(), "verify should pass: {verify}");
    assert_eq!(verify["data"]["matches"], json!(true));
    assert_eq!(verify["data"]["current_digest"], json!(digest));
    assert_eq!(verify["data"]["lock_digest"], json!(digest));
}

#[test]
fn skill_provenance_verify_detects_local_source_drift() {
    let root = TestDir::new("skill-provenance-drift");
    let source = TestDir::new("skill-provenance-drift-source");
    write_file(&source.path().join("SKILL.md"), "# demo\n\nversion one\n");

    let source_arg = source.path().to_str().expect("source path");
    assert!(
        run_loom(root.path(), &["skill", "add", source_arg, "--name", "demo"])
            .0
            .status
            .success()
    );
    let lock_before = read_json(&root.path().join("loom.lock"));
    let recorded = lock_before["skills"]["demo"]["digest"]
        .as_str()
        .expect("recorded digest")
        .to_string();

    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "# demo\n\nversion two\n",
    );

    let (output, verify) = run_loom(root.path(), &["skill", "provenance", "verify", "demo"]);
    assert!(output.status.success(), "verify should pass: {verify}");
    assert_eq!(verify["data"]["matches"], json!(false));
    assert_eq!(verify["data"]["recorded_digest"], json!(recorded));
    assert_ne!(verify["data"]["current_digest"], json!(recorded));
    assert_eq!(verify["data"]["lock_digest"], json!(recorded));
}

#[test]
fn skill_provenance_verify_reads_actual_lock_file() {
    let root = TestDir::new("skill-provenance-lock-drift");
    let source = TestDir::new("skill-provenance-lock-drift-source");
    write_file(&source.path().join("SKILL.md"), "# demo\n\nversion one\n");

    let source_arg = source.path().to_str().expect("source path");
    assert!(
        run_loom(root.path(), &["skill", "add", source_arg, "--name", "demo"])
            .0
            .status
            .success()
    );
    let mut lock = read_json(&root.path().join("loom.lock"));
    lock["skills"]["demo"]["digest"] = json!("sha256:badlock");
    write_file(
        &root.path().join("loom.lock"),
        &(serde_json::to_string_pretty(&lock).expect("serialize lock") + "\n"),
    );

    let (output, verify) = run_loom(root.path(), &["skill", "provenance", "verify", "demo"]);
    assert!(output.status.success(), "verify should pass: {verify}");
    assert_eq!(verify["data"]["matches"], json!(false));
    assert_eq!(verify["data"]["lock_present"], json!(true));
    assert_eq!(verify["data"]["lock_digest"], json!("sha256:badlock"));
    assert_ne!(
        verify["data"]["current_digest"], verify["data"]["lock_digest"],
        "verify must compare against the actual loom.lock file"
    );
}

#[test]
fn skill_provenance_refresh_updates_lock_without_projection_mutation() {
    let root = TestDir::new("skill-provenance-refresh");
    let source = TestDir::new("skill-provenance-refresh-source");
    let target = TestDir::new("skill-provenance-refresh-target");
    write_file(&source.path().join("SKILL.md"), "# demo\n\nversion one\n");

    let source_arg = source.path().to_str().expect("source path");
    assert!(
        run_loom(root.path(), &["skill", "add", source_arg, "--name", "demo"])
            .0
            .status
            .success()
    );
    let (output, env) = target_add(root.path(), "codex", target.path(), "managed");
    assert!(output.status.success(), "target add should pass: {env}");
    let target_id = env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();
    let (output, env) = binding_add(
        root.path(),
        "codex",
        "default",
        "path-prefix",
        "/tmp/demo-workspace",
        &target_id,
    );
    assert!(output.status.success(), "binding add should pass: {env}");
    let binding_id = env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id")
        .to_string();
    let (output, env) = skill_project(root.path(), "demo", &binding_id, Some("copy"));
    assert!(output.status.success(), "skill project should pass: {env}");

    let projections_before =
        fs::read_to_string(root.path().join("state/registry/projections.json"))
            .expect("read projections before");
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "# demo\n\nversion two\n",
    );

    let (output, refresh) = run_loom(root.path(), &["skill", "provenance", "refresh", "demo"]);
    assert!(output.status.success(), "refresh should pass: {refresh}");
    assert_eq!(refresh["data"]["changed"], json!(true));
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/projections.json"))
            .expect("read projections after"),
        projections_before,
        "provenance refresh must not rewrite projection state"
    );

    let (output, verify) = run_loom(root.path(), &["skill", "provenance", "verify", "demo"]);
    assert!(output.status.success(), "verify should pass: {verify}");
    assert_eq!(verify["data"]["matches"], json!(true));
    assert_eq!(
        verify["data"]["current_digest"],
        refresh["data"]["current_digest"]
    );
}

#[test]
fn skill_provenance_refresh_preserves_files_when_atomic_write_fails() {
    let root = TestDir::new("skill-provenance-refresh-atomic-failure");
    let source = TestDir::new("skill-provenance-refresh-atomic-failure-source");
    write_file(&source.path().join("SKILL.md"), "# demo\n\nversion one\n");

    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(root.path(), &["skill", "add", source_arg, "--name", "demo"]);
    assert!(output.status.success(), "skill add should pass: {env}");
    let sources_path = root.path().join("state/registry/sources.json");
    let lock_path = root.path().join("loom.lock");
    let sources_before = fs::read_to_string(&sources_path).expect("read sources before");
    let lock_before = fs::read_to_string(&lock_path).expect("read lock before");
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "# demo\n\nversion two\n",
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "write_atomic")],
        &["skill", "provenance", "refresh", "demo"],
    );

    assert!(!output.status.success(), "refresh should fail: {env}");
    assert_eq!(env["error"]["code"], json!("IO_ERROR"));
    assert_eq!(
        fs::read_to_string(&sources_path).expect("read sources after"),
        sources_before,
        "failed atomic sources write must preserve old file"
    );
    assert_eq!(
        fs::read_to_string(&lock_path).expect("read lock after"),
        lock_before,
        "failed atomic lock write must preserve old file"
    );
}

#[test]
fn skill_add_records_local_git_ref_commit_and_tree_hash() {
    let root = TestDir::new("skill-provenance-git");
    let source = TestDir::new("skill-provenance-git-source");
    let (v1, v2) = init_git_skill_repo(source.path());
    let source_arg = source.path().to_str().expect("source path");

    let (output, tag_env) = run_loom(
        root.path(),
        &[
            "skill", "add", source_arg, "--name", "demo-tag", "--ref", "v1", "--subdir", "skill",
        ],
    );
    assert!(output.status.success(), "tag import should pass: {tag_env}");

    let (output, branch_env) = run_loom(
        root.path(),
        &[
            "skill",
            "add",
            source_arg,
            "--name",
            "demo-branch",
            "--ref",
            "main",
            "--subdir",
            "skill",
        ],
    );
    assert!(
        output.status.success(),
        "branch import should pass: {branch_env}"
    );

    let (output, commit_env) = run_loom(
        root.path(),
        &[
            "skill",
            "add",
            source_arg,
            "--name",
            "demo-commit",
            "--ref",
            &v1,
            "--subdir",
            "skill",
        ],
    );
    assert!(
        output.status.success(),
        "commit import should pass: {commit_env}"
    );

    let sources = read_json(&root.path().join("state/registry/sources.json"));
    let records = sources["sources"].as_array().expect("sources array");
    let find = |skill: &str| {
        records
            .iter()
            .find(|record| record["skill_id"] == skill)
            .unwrap_or_else(|| panic!("missing record for {skill}"))
    };
    let tag = find("demo-tag");
    let branch = find("demo-branch");
    let commit = find("demo-commit");

    assert_eq!(tag["source"]["provider"], json!("git"));
    assert_eq!(tag["source"]["repository"], json!(source_arg));
    assert_eq!(tag["source"]["requested_ref"], json!("v1"));
    assert_eq!(tag["source"]["resolved_commit"], json!(v1));
    assert_eq!(tag["source"]["subdir"], json!("skill"));
    assert!(tag["source"]["tree_sha"].as_str().is_some());

    assert_eq!(branch["source"]["requested_ref"], json!("main"));
    assert_eq!(branch["source"]["resolved_commit"], json!(v2));
    assert_ne!(
        branch["artifact"]["digest"], tag["artifact"]["digest"],
        "branch import should reflect the newer skill contents"
    );

    assert_eq!(commit["source"]["requested_ref"], json!(v1));
    assert_eq!(commit["source"]["resolved_commit"], json!(v1));
    assert_eq!(
        commit["artifact"]["digest"], tag["artifact"]["digest"],
        "tag and pinned commit should import the same skill tree"
    );

    let lock = read_json(&root.path().join("loom.lock"));
    assert_eq!(lock["skills"]["demo-tag"]["commit"], json!(v1));
    assert_eq!(lock["skills"]["demo-tag"]["ref"], json!("v1"));
    assert_eq!(lock["skills"]["demo-branch"]["commit"], json!(v2));
    assert_eq!(lock["skills"]["demo-branch"]["ref"], json!("main"));
}

#[test]
fn provider_catalog_preview_and_install_dry_run_are_safe() {
    let root = TestDir::new("provider-cli-root");
    let catalog = TestDir::new("provider-cli-catalog");
    let marker = catalog.path().join("executed-marker");
    let skill = catalog.path().join("skills/demo");
    write_file(
        &skill.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo provider skill.\nlicense: MIT\n---\n# Demo\n",
    );
    write_file(
        &skill.join("scripts/run.sh"),
        &format!("#!/bin/sh\ntouch {}\n", marker.display()),
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "provider",
            "add",
            "corp-local",
            "--kind",
            "local",
            "--url",
            catalog.path().to_str().unwrap(),
        ],
    );
    assert!(output.status.success(), "provider add should pass: {env}");
    assert_eq!(env["data"]["provider"]["id"], "corp-local");

    let locator = format!("local:{}//skills/demo", catalog.path().display());
    let (output, preview) = run_loom(root.path(), &["catalog", "preview", &locator]);
    assert!(output.status.success(), "preview should pass: {preview}");
    assert!(!marker.exists(), "preview must not execute scripts");
    assert_eq!(preview["data"]["preview"]["metadata"]["name"], "demo");
    assert_eq!(
        preview["data"]["preview"]["scripts"][0]["path"],
        "scripts/run.sh"
    );
    let digest = preview["data"]["preview"]["provenance"]["digest"]
        .as_str()
        .expect("preview digest");

    let pinned = format!("{locator}@{digest}");
    let (output, dry_run) = run_loom(
        root.path(),
        &["skill", "install", &pinned, "--name", "demo", "--dry-run"],
    );
    assert!(
        output.status.success(),
        "install dry-run should pass: {dry_run}"
    );
    assert_eq!(
        dry_run["data"]["would_write"]["trust_record"]["trust"],
        "third-party-unreviewed"
    );
    assert_eq!(
        dry_run["data"]["would_write"]["provenance_record"]["artifact"]["digest"],
        digest
    );
    assert!(!root.path().join("skills/demo").exists());
    assert!(!root.path().join("state/registry/sources.json").exists());
    assert!(!root.path().join("loom.lock").exists());
    assert!(!root.path().join("state/registry/trust.json").exists());
    assert!(
        dry_run["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .filter_map(Value::as_str)
            .any(|action| action.contains("--agent <agent> --dry-run")),
        "install plan must provide a complete activation template: {dry_run}"
    );

    let (output, blocked) = run_loom(
        root.path(),
        &[
            "skill",
            "install",
            &locator,
            "--name",
            "demo",
            "--policy-profile",
            "strict",
            "--dry-run",
        ],
    );
    assert!(
        !output.status.success(),
        "unpinned install must fail: {blocked}"
    );
    assert_eq!(blocked["error"]["code"], "POLICY_BLOCKED");
}

#[test]
fn provider_install_apply_imports_pinned_local_skill_with_provenance_lock_and_trust() {
    let root = TestDir::new("provider-install-apply-root");
    let catalog = TestDir::new("provider-install-apply-catalog");
    let marker = catalog.path().join("executed-marker");
    let skill = catalog.path().join("skills/demo");
    write_file(
        &skill.join("SKILL.md"),
        "---\nname: demo\ndescription: Demo provider skill.\nlicense: MIT\n---\n# Demo\n",
    );
    write_file(
        &skill.join("scripts/run.sh"),
        &format!("#!/bin/sh\ntouch {}\n", marker.display()),
    );

    let locator = format!("local:{}//skills/demo", catalog.path().display());
    let (output, preview) = run_loom(root.path(), &["catalog", "preview", &locator]);
    assert!(output.status.success(), "preview should pass: {preview}");
    let digest = preview["data"]["preview"]["provenance"]["digest"]
        .as_str()
        .expect("preview digest")
        .to_string();

    let pinned = format!("{locator}@{digest}");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "install", &pinned, "--name", "demo"],
    );
    assert!(output.status.success(), "install apply should pass: {env}");
    assert_eq!(env["data"]["dry_run"], json!(false));
    assert_eq!(env["data"]["skill"], json!("demo"));
    let op_id = env["meta"]["op_id"].as_str().expect("install op_id");
    assert!(op_id.starts_with("op_"), "unexpected op_id: {op_id}");
    assert!(
        !marker.exists(),
        "provider install must not execute scripts"
    );
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(
        env["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .filter_map(Value::as_str)
            .any(|action| action.contains("--agent <agent> --dry-run")),
        "install result must provide a complete activation template: {env}"
    );

    let sources = read_json(&root.path().join("state/registry/sources.json"));
    let record = &sources["sources"][0];
    assert_eq!(record["skill_id"], json!("demo"));
    assert_eq!(record["source"]["provider"], json!("local"));
    assert_eq!(record["source"]["locator"], json!(pinned));
    assert_eq!(record["artifact"]["digest"], json!(digest));

    let lock = read_json(&root.path().join("loom.lock"));
    assert_eq!(lock["skills"]["demo"]["provider"], json!("local"));
    assert_eq!(lock["skills"]["demo"]["digest"], json!(digest));
    assert_eq!(lock["skills"]["demo"]["ref"], json!(digest));

    let trust = read_json(&root.path().join("state/registry/trust.json"));
    assert_eq!(trust["skills"][0]["skill_id"], json!("demo"));
    assert_eq!(trust["skills"][0]["trust"], json!("third-party-unreviewed"));
    assert_eq!(trust["skills"][0]["quarantined"], json!(false));

    let ops = operations_log(root.path());
    assert!(ops.contains(&format!(r#""op_id":"{op_id}""#)));
    assert!(ops.contains(r#""intent":"skill.install""#));

    let (output, verify) = run_loom(root.path(), &["skill", "provenance", "verify", "demo"]);
    assert!(output.status.success(), "verify should pass: {verify}");
    assert_eq!(verify["data"]["matches"], json!(true));
    assert_eq!(verify["data"]["current_digest"], json!(digest));
}

#[test]
fn provider_errors_are_typed_and_block_unsafe_inputs() {
    let root = TestDir::new("provider-cli-errors");

    let (output, env) = run_loom(
        root.path(),
        &[
            "provider",
            "add",
            "bad",
            "--kind",
            "github",
            "--url",
            "https://token@example.com/org/repo",
        ],
    );
    assert!(!output.status.success(), "credential url must fail: {env}");
    assert_eq!(env["error"]["code"], "ARG_INVALID");

    let (output, env) = run_loom(
        root.path(),
        &["catalog", "show", "missing:owner/repo//skill"],
    );
    assert!(
        !output.status.success(),
        "unknown provider must fail: {env}"
    );
    assert_eq!(env["error"]["code"], "PROVIDER_NOT_FOUND");

    let (output, env) = run_loom(root.path(), &["catalog", "show", "team:core/demo"]);
    assert!(
        !output.status.success(),
        "reserved team provider must fail: {env}"
    );
    assert_eq!(env["error"]["code"], "ARG_INVALID");

    let catalog = TestDir::new("provider-cli-danger");
    let skill = catalog.path().join("skills/danger");
    write_file(
        &skill.join("SKILL.md"),
        "---\nname: danger\ndescription: Dangerous demo.\n---\n# Danger\n",
    );
    write_file(&skill.join("scripts/danger.sh"), "#!/bin/sh\nrm -rf /\n");
    let locator = format!("local:{}//skills/danger", catalog.path().display());
    let (output, preview) = run_loom(root.path(), &["catalog", "preview", &locator]);
    assert!(
        output.status.success(),
        "danger preview should pass: {preview}"
    );
    let digest = preview["data"]["preview"]["provenance"]["digest"]
        .as_str()
        .unwrap();
    let pinned = format!("{locator}@{digest}");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "install", &pinned, "--name", "danger", "--dry-run"],
    );
    assert!(!output.status.success(), "critical scan must block: {env}");
    assert_eq!(env["error"]["code"], "POLICY_BLOCKED");
}
