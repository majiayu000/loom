use std::fs;
use std::process::Command;

mod common;

use common::TestDir;

#[test]
fn top_level_help_describes_command_groups() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--help")
        .output()
        .expect("run loom help");

    assert!(
        output.status.success(),
        "help unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Initialize the default registry and scan existing agent skill directories",
        "Import and update skills from observed targets",
        "Inspect and configure registry workspace state",
        "Register and inspect agent skill directories",
        "Manage skill sources, projections, and versions",
        "Synchronize the registry through its Git remote",
        "Inspect, replay, and repair operation history",
        "Serve the local registry control panel",
    ] {
        assert!(
            stdout.contains(expected),
            "help missing command description {expected:?}: {stdout}"
        );
    }
}

#[test]
fn top_level_init_uses_default_registry_root_and_scans_existing_dirs() {
    let home = TestDir::new("cli-default-home");
    let codex_skill = home.path().join(".codex/skills/demo-skill");
    fs::create_dir_all(&codex_skill).expect("create codex skill dir");
    fs::write(codex_skill.join("SKILL.md"), "# Demo\n").expect("write skill");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("init")
        .env("HOME", home.path())
        .env_remove("CODEX_SKILLS_DIR")
        .env_remove("CLAUDE_SKILLS_DIR")
        .output()
        .expect("run loom init");

    assert!(
        output.status.success(),
        "init unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse loom init json");
    assert_eq!(env["cmd"], serde_json::json!("init"));
    assert_eq!(env["data"]["scanned"], serde_json::json!(true));
    assert_eq!(
        env["data"]["imported"].as_array().map(|items| items.len()),
        Some(1)
    );
    assert!(
        home.path()
            .join(".loom-registry/state/registry/targets.json")
            .is_file()
    );
}

#[test]
fn json_output_defaults_to_compact_envelope() {
    let root = TestDir::new("cli-compact-json");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["workspace", "status"])
        .output()
        .expect("run loom status");

    assert!(
        output.status.success(),
        "status unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.lines().count(),
        1,
        "--json should be compact by default: {stdout}"
    );
    assert!(
        stdout.contains("\"error\":null"),
        "successful envelopes must keep a stable error:null field: {stdout}"
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("parse compact json");
}

#[test]
fn pretty_json_output_is_opt_in() {
    let root = TestDir::new("cli-pretty-json");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--pretty")
        .arg("--root")
        .arg(root.path())
        .args(["workspace", "status"])
        .output()
        .expect("run loom status");

    assert!(
        output.status.success(),
        "status unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().count() > 1,
        "--json --pretty should keep human-readable formatting: {stdout}"
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout).expect("parse pretty json");
}

#[test]
fn migrate_subcommand_is_removed() {
    let root = TestDir::new("cli-no-migrate");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["migrate", "legacy-to-registry", "--plan"])
        .output()
        .expect("run loom");

    assert!(
        !output.status.success(),
        "migrate unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("unrecognized subcommand")
            || stderr.contains("unexpected argument")
            || stderr.contains("found argument"),
        "stderr did not indicate migrate removal: {}",
        stderr
    );
}

#[test]
fn skill_orphan_clean_nested_command_is_available() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "orphan", "clean", "--help"])
        .output()
        .expect("run loom");

    assert!(
        output.status.success(),
        "orphan clean help failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--delete-live-paths"),
        "orphan clean help must expose explicit live-path deletion flag: {}",
        stdout
    );
}

#[test]
fn skill_monitor_observed_command_is_available() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "monitor-observed", "--help"])
        .output()
        .expect("run loom");

    assert!(
        output.status.success(),
        "monitor-observed help failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in ["--once", "--interval-seconds", "--target"] {
        assert!(
            stdout.contains(expected),
            "monitor-observed help missing {expected:?}: {stdout}"
        );
    }
}

#[test]
fn top_level_monitor_command_is_available() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["monitor", "--help"])
        .output()
        .expect("run loom");

    assert!(
        output.status.success(),
        "monitor help failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in ["--once", "--interval-seconds", "--target"] {
        assert!(
            stdout.contains(expected),
            "monitor help missing {expected:?}: {stdout}"
        );
    }
}
