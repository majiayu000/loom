mod common;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{TestDir, write_file};
use serde_json::Value;
use skillloom::cli_contract::{
    CLI_CONTRACT_VERSION, check_surface_inventory, contract_version_matches,
};

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
            "[compatibility]\ncli_contract = \">=1.9.0,<2.0.0\"\n",
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
        CLI_CONTRACT_VERSION,
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

#[cfg(unix)]
fn installer_fixture(name: &str) -> (TestDir, PathBuf, String, String) {
    let fixture = TestDir::new(name);
    let version = "9.8.7".to_string();
    let target = "fixture-target".to_string();
    let archive = format!("skillloom-{version}-{target}.tar.gz");
    let staging = fixture.path().join("staging");
    let bundle = staging.join(format!("skillloom-{version}-{target}"));
    let release = fixture
        .path()
        .join("releases/download")
        .join(format!("v{version}"));
    write_file(
        &bundle.join("loom"),
        "#!/bin/sh\nprintf 'loom fixture\\n'\n",
    );
    let mut permissions = fs::metadata(bundle.join("loom"))
        .expect("fixture binary metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(bundle.join("loom"), permissions).expect("fixture binary executable");
    write_file(
        &bundle.join("skills/loom-registry/SKILL.md"),
        "# Installer fixture\n",
    );
    write_file(
        &bundle.join("contracts/agent-command-surfaces.toml"),
        "[[surface]]\nid = \"fixture\"\n",
    );
    write_file(&bundle.join("contract-manifest.json"), "{}\n");
    fs::create_dir_all(&release).expect("release fixture directory");
    let archive_path = release.join(&archive);
    let tar = Command::new("tar")
        .args(["-C", staging.to_str().expect("staging path"), "-czf"])
        .arg(&archive_path)
        .arg(bundle.file_name().expect("bundle name"))
        .output()
        .expect("create installer archive");
    assert!(
        tar.status.success(),
        "tar failed: {}",
        String::from_utf8_lossy(&tar.stderr)
    );
    let checksum = Command::new("shasum")
        .args(["-a", "256"])
        .arg(&archive_path)
        .output()
        .expect("checksum installer archive");
    assert!(checksum.status.success());
    let digest = String::from_utf8(checksum.stdout)
        .expect("checksum output")
        .split_whitespace()
        .next()
        .expect("checksum digest")
        .to_string();
    write_file(
        &release.join("SHA256SUMS"),
        &format!("{digest}  {archive}\n"),
    );
    (fixture, release, version, target)
}

#[cfg(unix)]
#[test]
fn installer_verifies_and_installs_release_bundle() {
    let (fixture, release, version, target) = installer_fixture("release-installer-success");
    let bin_dir = fixture.path().join("installed/bin");
    let data_dir = fixture.path().join("installed/data");
    let base_url = format!(
        "file://{}",
        release
            .parent()
            .expect("download root")
            .to_str()
            .expect("download root path")
    );
    let output = Command::new("sh")
        .arg("scripts/install.sh")
        .args(["--version", &version, "--target", &target, "--bin-dir"])
        .arg(&bin_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .env("LOOM_INSTALL_BASE_URL", base_url)
        .output()
        .expect("run release installer");
    assert!(
        output.status.success(),
        "installer failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(bin_dir.join("loom").is_file());
    assert_eq!(
        fs::read_to_string(data_dir.join("current/skills/loom-registry/SKILL.md"))
            .expect("installed Skill"),
        "# Installer fixture\n"
    );
    assert!(
        data_dir
            .join("current/contracts/agent-command-surfaces.toml")
            .is_file()
    );
}

#[cfg(unix)]
#[test]
fn installer_fails_closed_on_checksum_mismatch() {
    let (fixture, release, version, target) = installer_fixture("release-installer-checksum");
    write_file(
        &release.join("SHA256SUMS"),
        &format!("{}  skillloom-{version}-{target}.tar.gz\n", "0".repeat(64)),
    );
    let bin_dir = fixture.path().join("installed/bin");
    let data_dir = fixture.path().join("installed/data");
    let base_url = format!(
        "file://{}",
        release
            .parent()
            .expect("download root")
            .to_str()
            .expect("download root path")
    );
    let output = Command::new("sh")
        .arg("scripts/install.sh")
        .args(["--version", &version, "--target", &target, "--bin-dir"])
        .arg(&bin_dir)
        .arg("--data-dir")
        .arg(&data_dir)
        .env("LOOM_INSTALL_BASE_URL", base_url)
        .output()
        .expect("run release installer");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum mismatch"));
    assert!(!bin_dir.join("loom").exists());
    assert!(!data_dir.exists());
}

#[test]
fn optional_publish_skips_are_visible_in_release_summary() {
    let workflow = include_str!("../.github/workflows/release.yml");
    for message in [
        "::warning title=crates.io publish skipped::",
        "::warning title=Homebrew publish skipped::",
    ] {
        assert!(
            workflow.contains(message),
            "missing workflow warning: {message}"
        );
    }
    assert!(workflow.matches("GITHUB_STEP_SUMMARY").count() >= 2);
    assert!(workflow.contains("Do not advertise"));
    assert!(workflow.contains("- name: Smoke public installer"));
    assert!(workflow.contains("scripts/install.sh"));
}

#[test]
fn distribution_readiness_accepts_complete_release_fixture() {
    let fixture = TestDir::new("distribution-readiness-complete");
    let readme = fixture.path().join("README.md");
    write_file(
        &readme,
        "curl -fsSL https://raw.githubusercontent.com/majiayu000/loom/main/scripts/install.sh | sh\n",
    );
    let release = fixture.path().join("release.json");
    write_file(
        &release,
        r#"{
          "tag_name": "v9.8.7",
          "draft": false,
          "assets": [
            {"name": "SHA256SUMS", "state": "uploaded", "size": 320},
            {"name": "skillloom-9.8.7-aarch64-apple-darwin.tar.gz", "state": "uploaded", "size": 1},
            {"name": "skillloom-9.8.7-x86_64-apple-darwin.tar.gz", "state": "uploaded", "size": 1},
            {"name": "skillloom-9.8.7-x86_64-unknown-linux-gnu.tar.gz", "state": "uploaded", "size": 1}
          ]
        }"#,
    );
    let output = Command::new("python3")
        .args([
            "scripts/distribution-readiness.py",
            "--tag",
            "v9.8.7",
            "--readme",
        ])
        .arg(&readme)
        .arg("--release-json")
        .arg(&release)
        .output()
        .expect("run distribution readiness fixture");
    assert!(
        output.status.success(),
        "readiness failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn readme_and_landing_advertise_the_verified_installer() {
    let installer =
        "curl -fsSL https://raw.githubusercontent.com/majiayu000/loom/main/scripts/install.sh | sh";
    assert!(include_str!("../README.md").contains(installer));
    assert!(include_str!("../panel/src/components/landing/Hero.tsx").contains(installer));
    assert!(!include_str!("../README.md").contains("brew install majiayu000/tap/loom"));
    assert!(
        !include_str!("../scripts/install.sh")
            .contains("install from source with 'cargo install skillloom'")
    );
}

#[test]
fn distribution_readiness_rejects_missing_release_asset() {
    let fixture = TestDir::new("distribution-readiness-missing-asset");
    let readme = fixture.path().join("README.md");
    write_file(
        &readme,
        "curl -fsSL https://raw.githubusercontent.com/majiayu000/loom/main/scripts/install.sh | sh\n",
    );
    let release = fixture.path().join("release.json");
    write_file(
        &release,
        r#"{"tag_name":"v9.8.7","draft":false,"assets":[{"name":"SHA256SUMS","state":"uploaded","size":320}]}"#,
    );
    let output = Command::new("python3")
        .args([
            "scripts/distribution-readiness.py",
            "--tag",
            "v9.8.7",
            "--readme",
        ])
        .arg(&readme)
        .arg("--release-json")
        .arg(&release)
        .output()
        .expect("run incomplete distribution fixture");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("release asset is missing"));
}

#[test]
#[ignore = "release workflow supplies an unpacked, verified native bundle"]
fn packaged_surface_fixture_matrix() {
    let bundle = PathBuf::from(
        std::env::var("LOOM_PACKAGED_CONTRACT_BUNDLE")
            .expect("release workflow must provide LOOM_PACKAGED_CONTRACT_BUNDLE"),
    );
    let binary = PathBuf::from(
        std::env::var("LOOM_PACKAGED_BINARY")
            .expect("release workflow must provide LOOM_PACKAGED_BINARY"),
    );
    assert_eq!(
        fs::read(bundle.join("contracts/agent-command-surfaces.toml"))
            .expect("read packaged surface inventory"),
        fs::read("docs/agent-command-surfaces.toml").expect("read source surface inventory")
    );
    let metadata = fs::read_to_string(bundle.join("skills/loom-registry/loom.skill.toml"))
        .expect("read packaged Skill metadata");
    assert!(metadata.contains("cli_contract = \">=1.9.0,<2.0.0\""));
    assert!(contract_version_matches(">=1.0.0,<2.0.0", CLI_CONTRACT_VERSION).unwrap());
    let report = check_surface_inventory(Path::new("."))
        .expect("run the complete parser-backed fixture matrix");

    for argv in report.parser_argv {
        let mut args = argv.into_iter().skip(1).collect::<Vec<_>>();
        args.push("--help".to_string());
        let parsed = Command::new(&binary)
            .args(&args)
            .output()
            .expect("run packaged binary parser fixture");
        assert!(
            parsed.status.success(),
            "packaged binary rejected parser fixture {args:?}: {}",
            String::from_utf8_lossy(&parsed.stderr)
        );
    }

    let root = TestDir::new("packaged-contract-native-binary");
    let output = Command::new(binary)
        .args(["--json", "--root"])
        .arg(root.path())
        .args(["workspace", "status"])
        .output()
        .expect("run packaged native binary");
    assert!(output.status.success(), "packaged binary status failed");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("parse packaged envelope");
    assert_eq!(envelope["cli_contract_version"], CLI_CONTRACT_VERSION);
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
    assert_eq!(data["cli_contract_version"], CLI_CONTRACT_VERSION);
    assert_eq!(data["skill_cli_contract_range"], ">=1.9.0,<2.0.0");
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

#[cfg(unix)]
#[test]
fn homebrew_tap_first_release_removes_untracked_formula_before_checkout() {
    let fixture = TestDir::new("release-contract-homebrew-first-formula");
    let tap = fixture.path().join("tap");
    let tap_text = tap.to_str().expect("tap path");
    git(fixture.path(), &["init", "-q", tap_text]);
    git(&tap, &["config", "user.email", "release@example.invalid"]);
    git(&tap, &["config", "user.name", "Release Fixture"]);
    write_file(&tap.join("README.md"), "tap fixture\n");
    git(&tap, &["add", "README.md"]);
    git(&tap, &["commit", "-qm", "initialize tap"]);
    write_file(&tap.join("Formula/loom.rb"), "version \"first-release\"\n");

    let workflow = include_str!("../.github/workflows/release.yml");
    let guard_start = workflow
        .find("if git ls-files --error-unmatch -- Formula/loom.rb")
        .expect("workflow must detect whether the generated formula is tracked");
    let guard_end = workflow[guard_start..]
        .find("if git ls-remote --exit-code --heads origin")
        .map(|offset| guard_start + offset)
        .expect("formula cleanup must precede release-branch lookup");
    let cleanup_guard = &workflow[guard_start..guard_end];

    let status = Command::new("bash")
        .args(["-c", &format!("set -euo pipefail\n{cleanup_guard}")])
        .current_dir(&tap)
        .status()
        .expect("run first-release formula cleanup guard");
    assert!(status.success());
    assert!(
        !tap.join("Formula/loom.rb").exists(),
        "an untracked generated formula must be removed before branch checkout"
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
    let tracked_formula_guard = workflow
        .find("git ls-files --error-unmatch -- Formula/loom.rb")
        .expect("workflow must distinguish tracked and first-release formulas");
    let tracked_formula_restore = workflow
        .find("git restore -- Formula/loom.rb")
        .expect("workflow must restore a tracked formula before switching branches");
    let untracked_formula_remove = workflow
        .find("rm -f Formula/loom.rb")
        .expect("workflow must remove an untracked first-release formula");
    assert!(tracked_formula_guard < tracked_formula_restore);
    assert!(tracked_formula_guard < untracked_formula_remove);
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
