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
