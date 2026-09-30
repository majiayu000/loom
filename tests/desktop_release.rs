#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use common::{TestDir, write_file};
use yaml_rust2::{Yaml, YamlLoader};

fn workflow_steps() -> Vec<Yaml> {
    YamlLoader::load_from_str(include_str!("../.github/workflows/desktop-release.yml"))
        .expect("desktop workflow YAML")[0]["jobs"]["macos"]["steps"]
        .as_vec()
        .expect("desktop workflow steps")
        .clone()
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .expect("fixture git");
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout)
        .expect("git output UTF-8")
        .trim()
        .to_string()
}

fn run_identity(event: &str, tag: &str, release_status: &str) -> (Output, String, String) {
    let fixture = TestDir::new("desktop-release");
    let remote = fixture.path().join("remote");
    let checkout = fixture.path().join("checkout");
    fs::create_dir_all(&remote).expect("remote directory");
    fs::create_dir_all(&checkout).expect("checkout directory");
    git(&remote, &["init", "-b", "main"]);
    git(&remote, &["config", "user.name", "Fixture"]);
    git(
        &remote,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(
        &remote,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "old",
        ],
    );
    git(&remote, &["tag", "-a", "v0.1.0", "-m", "old release"]);
    git(
        &remote,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "current",
        ],
    );
    git(&remote, &["tag", "v0.2.0"]);
    git(&remote, &["tag", "-a", "v0.3.0", "-m", "annotated release"]);
    let sha = git(&remote, &["rev-parse", "HEAD"]);
    git(&checkout, &["init"]);
    git(
        &checkout,
        &[
            "remote",
            "add",
            "origin",
            remote.to_str().expect("remote path"),
        ],
    );
    git(
        &checkout,
        &["fetch", "--depth=1", "--no-tags", "origin", "main"],
    );
    git(&checkout, &["checkout", "--detach", "FETCH_HEAD"]);

    let fake_bin = fixture.path().join("bin");
    let gh = fake_bin.join("gh");
    write_file(
        &gh,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$GH_CALLS\"\nexit \"$RELEASE_STATUS\"\n",
    );
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("executable gh");
    let output_path = fixture.path().join("output");
    let calls_path = fixture.path().join("gh-calls");
    write_file(&output_path, "");
    write_file(&calls_path, "");
    let script = workflow_steps()
        .iter()
        .take_while(|step| step["name"].as_str() != Some("Require Apple signing secrets"))
        .filter_map(|step| step["run"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let output = Command::new("bash")
        .args(["-c", &script])
        .current_dir(&checkout)
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_bin.display(),
                std::env::var("PATH").expect("PATH")
            ),
        )
        .env("GITHUB_EVENT_NAME", event)
        .env("GITHUB_SHA", sha)
        .env("GITHUB_REF_NAME", "v0.2.0")
        .env("GITHUB_REPOSITORY", "fixture/loom")
        .env("GITHUB_OUTPUT", &output_path)
        .env("RELEASE_TAG", tag)
        .env("RELEASE_STATUS", release_status)
        .env("GH_CALLS", &calls_path)
        .output()
        .expect("run workflow identity step");
    (
        output,
        fs::read_to_string(output_path).expect("workflow output"),
        fs::read_to_string(calls_path).expect("release queries"),
    )
}

#[test]
fn dispatch_rejects_tag_for_another_commit() {
    let (output, published, calls) = run_identity("workflow_dispatch", "v0.1.0", "0");
    assert!(
        !output.status.success(),
        "wrong commit accepted: {output:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not match"));
    assert!(published.is_empty());
    assert!(calls.is_empty());
}

#[test]
fn dispatch_rejects_missing_tag() {
    let (output, published, calls) = run_identity("workflow_dispatch", "v9.9.9", "0");
    assert!(!output.status.success(), "missing tag accepted: {output:?}");
    assert!(published.is_empty());
    assert!(calls.is_empty());
}

#[test]
fn dispatch_rejects_missing_release_and_api_failure() {
    for status in ["1", "2"] {
        let (output, published, calls) = run_identity("workflow_dispatch", "v0.2.0", status);
        assert!(
            !output.status.success(),
            "release query failure accepted: {output:?}"
        );
        assert!(published.is_empty());
        assert_eq!(calls, "release view v0.2.0 --repo fixture/loom\n");
    }
}

#[test]
fn dispatch_accepts_matching_lightweight_and_annotated_tags() {
    for tag in ["v0.2.0", "v0.3.0"] {
        let (output, published, calls) = run_identity("workflow_dispatch", tag, "0");
        assert!(output.status.success(), "matching tag failed: {output:?}");
        assert_eq!(published, format!("release_tag={tag}\n"));
        assert_eq!(calls, format!("release view {tag} --repo fixture/loom\n"));
    }
}

#[test]
fn blank_dispatch_only_builds_an_artifact() {
    let (output, published, calls) = run_identity("workflow_dispatch", "", "1");
    assert!(
        output.status.success(),
        "artifact-only dispatch failed: {output:?}"
    );
    assert!(published.is_empty());
    assert!(calls.is_empty());
}

#[test]
fn tag_push_can_create_a_release_at_the_pushed_commit() {
    let (output, published, calls) = run_identity("push", "", "1");
    assert!(output.status.success(), "tag push failed: {output:?}");
    assert_eq!(published, "release_tag=v0.2.0\n");
    assert!(calls.is_empty());
}

#[test]
fn publish_uses_only_the_verified_identity() {
    let steps = workflow_steps();
    let identity = steps
        .iter()
        .position(|step| step["id"].as_str() == Some("release_identity"))
        .expect("identity step");
    let secrets = steps
        .iter()
        .position(|step| step["name"].as_str() == Some("Require Apple signing secrets"))
        .expect("secrets step");
    assert!(identity < secrets);
    assert_eq!(
        steps[identity]["env"]["RELEASE_TAG"].as_str(),
        Some("${{ github.event.inputs.release_tag }}")
    );
    assert_eq!(
        steps[identity]["env"]["GH_TOKEN"].as_str(),
        Some("${{ github.token }}")
    );
    let publish = steps
        .iter()
        .find(|step| step["name"].as_str() == Some("Publish GitHub Release"))
        .expect("publish step");
    assert_eq!(
        publish["if"].as_str(),
        Some("steps.release_identity.outputs.release_tag != ''")
    );
    assert_eq!(
        publish["with"]["tag_name"].as_str(),
        Some("${{ steps.release_identity.outputs.release_tag }}")
    );
    assert_eq!(
        publish["with"]["target_commitish"].as_str(),
        Some("${{ github.event_name == 'push' && github.sha || '' }}")
    );
    assert_eq!(publish["with"]["overwrite_files"].as_bool(), Some(true));
}
