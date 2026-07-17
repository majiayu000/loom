mod common;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{TestDir, write_file};
use serde_json::Value;

struct Fixture {
    root: TestDir,
    binary: PathBuf,
    skill: PathBuf,
    inventory: PathBuf,
    output: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = TestDir::new(name);
        let binary = root.path().join("input/loom");
        let skill = root.path().join("input/loom-registry");
        let inventory = root.path().join("input/agent-command-surfaces.toml");
        let output = root.path().join("published/bundle");
        write_file(&binary, "#!/bin/sh\nexit 0\n");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&binary)
                .expect("binary metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&binary, permissions).expect("binary executable");
        }
        write_file(
            &skill.join("loom.skill.toml"),
            "[compatibility]\ncli_contract = \">=1.0.0,<2.0.0\"\n",
        );
        write_file(&skill.join("SKILL.md"), "# Loom registry\n");
        write_file(&inventory, "[[surface]]\nid = \"fixture\"\n");
        Self {
            root,
            binary,
            skill,
            inventory,
            output,
        }
    }

    fn publish(&self) -> Output {
        publish_command(self)
            .output()
            .expect("publish contract bundle")
    }

    fn verify(&self) -> Output {
        Command::new("python3")
            .args([
                "scripts/release-contract.py",
                "verify",
                "--bundle",
                self.output.to_str().expect("output path"),
            ])
            .output()
            .expect("verify contract bundle")
    }
}

fn publish_command(fixture: &Fixture) -> Command {
    let mut command = Command::new("python3");
    command.args([
        "scripts/release-contract.py",
        "publish",
        "--binary",
        fixture.binary.to_str().expect("binary path"),
        "--skill-dir",
        fixture.skill.to_str().expect("skill path"),
        "--inventory",
        fixture.inventory.to_str().expect("inventory path"),
        "--output-dir",
        fixture.output.to_str().expect("output path"),
        "--release-version",
        "0.1.5",
        "--contract-version",
        "1.0.0",
        "--target",
        "fixture-target",
    ]);
    command
}

fn manifest(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read manifest")).expect("parse manifest")
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("run git fixture command");
    assert!(
        output.status.success(),
        "git {args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git fixture output must be UTF-8")
        .trim()
        .to_string()
}

#[test]
fn packaged_contract_mismatch_fails() {
    let fixture = Fixture::new("release-contract-mismatch");
    assert!(fixture.publish().status.success());
    write_file(
        &fixture.output.join("skills/loom-registry/SKILL.md"),
        "changed\n",
    );
    assert!(!fixture.verify().status.success());
}

#[test]
fn packaged_contract_incompatible_skill_range_fails() {
    let fixture = Fixture::new("release-contract-incompatible-range");
    write_file(
        &fixture.skill.join("loom.skill.toml"),
        "[compatibility]\ncli_contract = \">=2.0.0,<3.0.0\"\n",
    );
    assert!(!fixture.publish().status.success());
    assert!(!fixture.output.exists());
}

#[test]
fn packaged_contract_invalid_semver_fails() {
    let fixture = Fixture::new("release-contract-invalid-semver");
    let output = publish_command(&fixture)
        .args(["--contract-version", "1.0"])
        .output()
        .expect("invalid contract publisher");
    assert!(!output.status.success());
    assert!(!fixture.output.exists());
}

#[test]
fn packaged_contract_digests_match() {
    let fixture = Fixture::new("release-contract-digests");
    assert!(fixture.publish().status.success());
    assert!(fixture.verify().status.success());
    let data = manifest(&fixture.output.join("contract-manifest.json"));
    assert_eq!(data["cli_contract_version"], "1.0.0");
    assert_eq!(data["skill_cli_contract_range"], ">=1.0.0,<2.0.0");
    for key in ["binary_sha256", "skill_tree_digest", "inventory_sha256"] {
        assert!(
            data[key]
                .as_str()
                .is_some_and(|value| value.starts_with("sha256:"))
        );
    }
}

#[test]
fn homebrew_share_contract_matches() {
    let mut fixture = Fixture::new("release-contract-homebrew");
    fixture.output = fixture.root.path().join("Cellar/loom/0.1.5/share/loom");
    assert!(fixture.publish().status.success());
    assert!(fixture.verify().status.success());
    assert!(
        fixture
            .output
            .join("skills/loom-registry/SKILL.md")
            .is_file()
    );
    assert!(
        fixture
            .output
            .join("contracts/agent-command-surfaces.toml")
            .is_file()
    );
}

#[test]
fn homebrew_tap_rerun_fast_forwards_existing_branch() {
    let fixture = TestDir::new("release-contract-homebrew-rerun");
    let remote = fixture.path().join("tap.git");
    let tap = fixture.path().join("tap");
    let remote_text = remote.to_str().expect("remote path");
    let tap_text = tap.to_str().expect("tap path");

    git(fixture.path(), &["init", "-q", "--bare", remote_text]);
    git(fixture.path(), &["init", "-q", tap_text]);
    git(&tap, &["config", "user.email", "release@example.invalid"]);
    git(&tap, &["config", "user.name", "Release Fixture"]);
    git(&tap, &["branch", "-M", "main"]);
    write_file(&tap.join("Formula/loom.rb"), "version \"main\"\n");
    git(&tap, &["add", "Formula/loom.rb"]);
    git(&tap, &["commit", "-qm", "main formula"]);
    git(&tap, &["remote", "add", "origin", remote_text]);
    git(&tap, &["push", "-u", "origin", "main"]);

    let branch = "loom-v0.1.5";
    git(&tap, &["checkout", "-qb", branch]);
    write_file(&tap.join("Formula/loom.rb"), "version \"old-release\"\n");
    git(&tap, &["add", "Formula/loom.rb"]);
    git(&tap, &["commit", "-qm", "old release formula"]);
    git(&tap, &["push", "-u", "origin", branch]);
    let old_head = git(&tap, &["rev-parse", "HEAD"]);

    git(&tap, &["checkout", "-q", "main"]);
    write_file(&tap.join("Formula/loom.rb"), "version \"new-release\"\n");
    let saved_formula = fixture.path().join("generated-loom.rb");
    fs::copy(tap.join("Formula/loom.rb"), &saved_formula).expect("save generated formula");
    git(&tap, &["restore", "--", "Formula/loom.rb"]);
    git(
        &tap,
        &[
            "fetch",
            "origin",
            "refs/heads/loom-v0.1.5:refs/remotes/origin/loom-v0.1.5",
        ],
    );
    git(&tap, &["checkout", "-qB", branch, "origin/loom-v0.1.5"]);
    fs::copy(&saved_formula, tap.join("Formula/loom.rb")).expect("restore generated formula");
    git(&tap, &["add", "Formula/loom.rb"]);
    git(&tap, &["commit", "-qm", "updated release formula"]);
    git(&tap, &["push", "origin", "HEAD:refs/heads/loom-v0.1.5"]);

    let new_head = git(&tap, &["rev-parse", "HEAD"]);
    git(&tap, &["merge-base", "--is-ancestor", &old_head, &new_head]);
    assert_eq!(
        fs::read_to_string(tap.join("Formula/loom.rb")).expect("read updated formula"),
        "version \"new-release\"\n"
    );
    assert!(git(&tap, &["status", "--porcelain"]).is_empty());

    let workflow = include_str!("../.github/workflows/release.yml");
    let no_op_guard = workflow
        .find("git diff --quiet \"origin/main\" -- Formula/loom.rb")
        .expect("workflow must compare Formula with tap main");
    let pr_lookup = workflow
        .find("pr_number=\"$(gh pr list")
        .expect("workflow must look up the tap PR");
    assert!(
        no_op_guard < pr_lookup,
        "no-op guard must precede PR creation"
    );

    git(&tap, &["checkout", "-q", "main"]);
    write_file(&tap.join("Formula/loom.rb"), "version \"new-release\"\n");
    git(&tap, &["add", "Formula/loom.rb"]);
    git(&tap, &["commit", "-qm", "main catches up"]);
    git(&tap, &["push", "origin", "main"]);
    git(&tap, &["checkout", "-q", branch]);
    git(
        &tap,
        &[
            "fetch",
            "origin",
            "refs/heads/main:refs/remotes/origin/main",
        ],
    );
    git(
        &tap,
        &["diff", "--quiet", "origin/main", "--", "Formula/loom.rb"],
    );
}

#[cfg(unix)]
#[test]
fn homebrew_tap_diff_error_fails_before_pr_lookup() {
    let fixture = TestDir::new("release-contract-homebrew-diff-error");
    let bin = fixture.path().join("bin");
    let gh_called = fixture.path().join("gh-called");
    write_file(
        &bin.join("git"),
        "#!/bin/sh\ncase \"$1\" in\n  fetch) exit 0 ;;\n  diff) exit 2 ;;\n  *) exit 0 ;;\nesac\n",
    );
    write_file(&bin.join("gh"), "#!/bin/sh\ntouch \"$GH_CALLED\"\nexit 0\n");
    for executable in [bin.join("git"), bin.join("gh")] {
        let mut permissions = fs::metadata(&executable)
            .expect("fake command metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).expect("make fake command executable");
    }

    let workflow = include_str!("../.github/workflows/release.yml");
    let guard_start = workflow
        .find("git fetch origin \"refs/heads/main:refs/remotes/origin/main\"")
        .expect("workflow must fetch tap main");
    let guard_end = workflow[guard_start..]
        .find("pr_number=\"$(gh pr list")
        .map(|offset| guard_start + offset)
        .expect("workflow must look up the tap PR after the guard");
    let guard = &workflow[guard_start..guard_end];
    assert!(guard.contains("formula_diff_status=$?"));
    assert!(guard.contains("[[ \"$formula_diff_status\" -ne 1 ]]"));

    let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .expect("fake command PATH");
    let status = Command::new("bash")
        .args(["-c", &format!("set -euo pipefail\n{guard}\ngh pr list")])
        .env("PATH", path)
        .env("GH_CALLED", &gh_called)
        .status()
        .expect("run Homebrew no-op guard");
    assert_eq!(status.code(), Some(2));
    assert!(
        !gh_called.exists(),
        "a git diff error must stop before any GitHub action"
    );
}

#[test]
fn release_manifest_is_atomic_and_untracked() {
    let fixture = Fixture::new("release-contract-atomic");
    let source_before = fs::read(&fixture.inventory).expect("source inventory");
    assert!(fixture.publish().status.success());
    assert_eq!(
        source_before,
        fs::read(&fixture.inventory).expect("source inventory after")
    );
    assert!(fixture.output.join("contract-manifest.json").is_file());
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(fixture.output.join("contract-manifest.json"))
            .expect("manifest metadata")
            .permissions()
            .mode()
            & 0o222,
        0
    );
}

#[test]
fn release_manifest_concurrent_publish() {
    let fixture = Fixture::new("release-contract-concurrent");
    let first = publish_command(&fixture).spawn().expect("first publisher");
    let second = publish_command(&fixture).spawn().expect("second publisher");
    assert!(
        first
            .wait_with_output()
            .expect("first result")
            .status
            .success()
    );
    assert!(
        second
            .wait_with_output()
            .expect("second result")
            .status
            .success()
    );
    assert!(fixture.verify().status.success());
}

#[test]
fn release_manifest_cancel_before_publish() {
    let fixture = Fixture::new("release-contract-cancel");
    let output = publish_command(&fixture)
        .env("LOOM_RELEASE_CONTRACT_FAULT", "before_publish")
        .output()
        .expect("faulted publisher");
    assert!(!output.status.success());
    assert!(!fixture.output.exists());
}

#[test]
fn packaged_contract_missing_inputs_fail_closed() {
    let inventory = Fixture::new("release-contract-missing-inventory");
    fs::remove_file(&inventory.inventory).expect("remove inventory");
    assert!(!inventory.publish().status.success());
    assert!(!inventory.output.exists());

    let binary = Fixture::new("release-contract-missing-binary");
    fs::remove_file(&binary.binary).expect("remove binary");
    assert!(!binary.publish().status.success());
    assert!(!binary.output.exists());

    let metadata = Fixture::new("release-contract-missing-metadata");
    fs::remove_file(metadata.skill.join("loom.skill.toml")).expect("remove metadata");
    assert!(!metadata.publish().status.success());
    assert!(!metadata.output.exists());

    let manifest = Fixture::new("release-contract-missing-manifest");
    assert!(manifest.publish().status.success());
    fs::remove_file(manifest.output.join("contract-manifest.json")).expect("remove manifest");
    assert!(!manifest.verify().status.success());
}
