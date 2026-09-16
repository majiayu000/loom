#![cfg(unix)]
mod common;

use common::{TestDir, run_loom, run_loom_with_env, write_file};
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn search_uses_explicit_provider_and_reports_real_results_and_failures() {
    let root = TestDir::new("remote-search");
    let bin = TestDir::new("fake-gh");
    let gh = bin.path().join("gh");
    let payload = json!({"total_count":42,"incomplete_results":true,"items":[{"path":"skills/demo/SKILL.md","html_url":"https://github.com/acme/demo/blob/main/skills/demo/SKILL.md","repository":{"full_name":"acme/demo","description":"Demo"}}]});
    write_file(&bin.path().join("response.json"), &payload.to_string());
    write_file(
        &gh,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$GH_TEST_ARGS\"\ncat \"$GH_TEST_RESPONSE\"\n",
    );
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap()
    );
    let args_path = bin.path().join("args");
    let response_path = bin.path().join("response.json");
    let envs = [
        ("PATH", path.as_str()),
        ("GH_TEST_ARGS", args_path.to_str().unwrap()),
        ("GH_TEST_RESPONSE", response_path.to_str().unwrap()),
    ];
    let (out, value) = run_loom(
        root.path(),
        &[
            "provider",
            "add",
            "corp",
            "--kind",
            "github",
            "--url",
            "https://github.com",
        ],
    );
    assert!(out.status.success(), "{value}");
    let state_before = fs::read(root.path().join("state/registry/providers.json")).unwrap();
    let (out, value) = run_loom_with_env(
        root.path(),
        &envs,
        &[
            "catalog",
            "search",
            "foo; echo not-a-shell",
            "--provider",
            "corp",
        ],
    );
    assert!(!out.status.success(), "{value}");
    assert!(!args_path.exists());
    let (out, value) = run_loom_with_env(
        root.path(),
        &envs,
        &[
            "catalog",
            "search",
            "foo; echo not-a-shell",
            "--provider",
            "corp",
            "--allow-network",
        ],
    );
    assert!(out.status.success(), "{value}");
    assert_eq!(
        value["data"]["results"][0]["locator"],
        "corp:acme/demo//skills/demo"
    );
    assert_eq!(value["data"]["results"][0]["source"]["pinned"], false);
    assert_eq!(value["data"]["truncated"], true);
    assert_eq!(value["data"]["incomplete_results"], true);
    let args = fs::read_to_string(&args_path).unwrap();
    assert!(args.contains("q=foo; echo not-a-shell filename:SKILL.md\n"));
    assert!(args.contains("--hostname\ngithub.com\n"));
    assert_eq!(
        state_before,
        fs::read(root.path().join("state/registry/providers.json")).unwrap()
    );
    write_file(&response_path, "{}");
    let (out, value) = run_loom_with_env(
        root.path(),
        &envs,
        &[
            "catalog",
            "search",
            "demo",
            "--provider",
            "corp",
            "--allow-network",
        ],
    );
    assert!(!out.status.success());
    assert_eq!(value["error"]["code"], "SCHEMA_MISMATCH");
    write_file(
        &gh,
        "#!/bin/sh\necho 'authentication required' >&2\nexit 1\n",
    );
    let (out, value) = run_loom_with_env(
        root.path(),
        &envs,
        &[
            "catalog",
            "search",
            "demo",
            "--provider",
            "corp",
            "--allow-network",
        ],
    );
    assert!(!out.status.success());
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("authentication required")
    );
}

#[test]
fn fetched_preview_resolves_commit_inspects_content_without_install_or_execution() {
    let root = TestDir::new("preview-registry");
    let source = TestDir::new("preview-source");
    write_file(
        &source.path().join("SKILL.md"),
        "---\nname: demo\ndescription: Demo remote skill.\n---\n# Demo\n",
    );
    let marker = source.path().join("executed");
    write_file(
        &source.path().join("scripts/test.sh"),
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );
    git(source.path(), &["init", "-q"]);
    git(source.path(), &["add", "."]);
    git(
        source.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let commit = git(source.path(), &["rev-parse", "HEAD"]);
    let key = format!("url.file://{}/.insteadOf", source.path().display());
    let envs = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", key.as_str()),
        ("GIT_CONFIG_VALUE_0", "https://github.com/acme/demo.git"),
    ];
    let (out, value) = run_loom_with_env(
        root.path(),
        &envs,
        &["catalog", "preview", "github:acme/demo"],
    );
    assert!(out.status.success(), "{value}");
    assert_eq!(value["data"]["source"]["resolved_commit"], commit);
    assert_eq!(
        value["data"]["resolved_locator"],
        format!("github:acme/demo@{commit}")
    );
    assert_eq!(value["data"]["preview"]["metadata"]["name"], "demo");
    assert_eq!(value["data"]["preview"]["lint"]["valid"], true);
    assert_ne!(value["data"]["preview"]["safety"]["status"], "not_run");
    assert_eq!(value["data"]["scripts_executed"], false);
    assert!(!root.path().join("skills/demo").exists());
    assert!(!root.path().join("state/registry/sources.json").exists());
    assert!(!marker.exists());
    assert_eq!(git(source.path(), &["status", "--porcelain"]), "");
    let (out, value) = run_loom_with_env(
        root.path(),
        &envs,
        &["catalog", "preview", "github:acme/demo@missing-ref"],
    );
    assert!(!out.status.success(), "{value}");
    assert!(!marker.exists());
}
