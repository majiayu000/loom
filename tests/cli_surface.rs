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
        "Export, inspect, and restore portable registry backups",
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
fn cli_contract_docs_track_current_surface() {
    let contract = include_str!("../docs/LOOM_CLI_CONTRACT.md");
    let readme = include_str!("../README.md");

    for command in [
        "`init`",
        "`backup`",
        "`monitor`",
        "`workspace`",
        "`target`",
        "`skill`",
        "`sync`",
        "`ops`",
        "`agent`",
        "`panel`",
        "`doctor`",
    ] {
        assert!(
            contract.contains(command),
            "CLI contract missing top-level command {command}"
        );
    }

    for stale in [
        "workspace status [--binding",
        "workspace doctor [--binding",
        "--all-bindings",
    ] {
        assert!(
            !contract.contains(stale),
            "CLI contract still documents stale workspace selector {stale:?}"
        );
    }

    for code in [
        "ARG_INVALID",
        "DEPENDENCY_CONFLICT",
        "SCHEMA_MISMATCH",
        "STATE_CORRUPT",
        "SKILL_NOT_FOUND",
        "BINDING_NOT_FOUND",
        "TARGET_NOT_FOUND",
        "TARGET_NOT_MANAGED",
        "TARGET_AGENT_MISMATCH",
        "PROJECTION_CONFLICT",
        "PROJECTION_METHOD_UNSUPPORTED",
        "CAPTURE_CONFLICT",
        "AUDIT_ERROR",
        "LOCK_BUSY",
        "REMOTE_UNREACHABLE",
        "REMOTE_DIVERGED",
        "PUSH_REJECTED",
        "REPLAY_CONFLICT",
        "QUEUE_BLOCKED",
        "GIT_ERROR",
        "IO_ERROR",
        "INTERNAL_ERROR",
    ] {
        assert!(
            contract.contains(code),
            "CLI contract missing error code {code}"
        );
    }

    for command in [
        "loom backup export",
        "loom backup inspect",
        "loom backup restore",
        "loom doctor",
        "loom skill history",
        "loom skill trash add",
        "loom skill trash list",
        "loom skill trash restore",
        "loom skill trash purge",
        "loom skill watch",
    ] {
        assert!(
            readme.contains(command),
            "README CLI reference missing command {command}"
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

    assert!(
        output.stderr.is_empty(),
        "--json parse failures should not write text stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse migrate removal json");
    assert_eq!(env["ok"], serde_json::json!(false));
    assert_eq!(env["cmd"], serde_json::json!("cli.parse"));
    assert_eq!(env["error"]["code"], serde_json::json!("ARG_INVALID"));
    assert_eq!(env["data"], serde_json::json!({}));
}

#[test]
fn json_mode_wraps_clap_value_errors() {
    let root = TestDir::new("cli-json-bad-agent");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--request-id")
        .arg("req-bad-agent")
        .arg("--root")
        .arg(root.path())
        .args([
            "target",
            "add",
            "--agent",
            "bad-agent",
            "--path",
            "/tmp/skills",
        ])
        .output()
        .expect("run loom");

    assert!(
        !output.status.success(),
        "invalid agent unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "--json value errors should not write text stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse invalid agent json");
    assert_eq!(env["ok"], serde_json::json!(false));
    assert_eq!(env["cmd"], serde_json::json!("cli.parse"));
    assert_eq!(env["request_id"], serde_json::json!("req-bad-agent"));
    assert_eq!(env["error"]["code"], serde_json::json!("ARG_INVALID"));
    assert_eq!(env["data"], serde_json::json!({}));
}

#[test]
fn json_parse_error_ignores_missing_request_id_value() {
    let root = TestDir::new("cli-json-missing-request-id");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--request-id")
        .arg("--root")
        .arg(root.path())
        .args(["workspace", "status"])
        .output()
        .expect("run loom");

    assert!(
        !output.status.success(),
        "missing request id unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "--json parse failures should not write text stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse missing request id json");
    assert_eq!(env["ok"], serde_json::json!(false));
    assert_eq!(env["cmd"], serde_json::json!("cli.parse"));
    assert_ne!(env["request_id"], serde_json::json!("--root"));
    assert!(
        env["request_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "parse failure must fall back to a generated request_id: {env}"
    );
    assert_eq!(env["error"]["code"], serde_json::json!("ARG_INVALID"));
}

#[test]
fn json_mode_ignores_empty_request_id_value() {
    let root = TestDir::new("cli-json-empty-request-id");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--request-id=")
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

    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse empty request id json");
    assert_eq!(env["ok"], serde_json::json!(true));
    assert!(
        env["request_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "empty request id must fall back to a generated request_id: {env}"
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
fn skill_orphan_list_nested_command_is_available() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "orphan", "list", "--help"])
        .output()
        .expect("run loom");

    assert!(
        output.status.success(),
        "orphan list help failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
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
fn top_level_version_flag_prints_cargo_pkg_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--version")
        .output()
        .expect("run loom --version");

    assert!(
        output.status.success(),
        "--version unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "--version output must contain CARGO_PKG_VERSION ({}): {stdout}",
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn skill_help_describes_every_subcommand() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "--help"])
        .output()
        .expect("run loom skill --help");

    assert!(
        output.status.success(),
        "skill help unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Import a skill source into the registry",
        "Project a registry skill into a bound target",
        "Capture live projection edits back to the source",
        "Commit source changes for one skill",
        "Create a version snapshot for one skill",
        "Tag a skill release",
        "Roll back a skill source to an earlier revision",
        "Diff two revisions of a skill source",
        "Continuously import and update skills from observed targets",
        "Run one import pass over observed targets and exit",
        "Inspect and clean projections orphaned by binding removal",
    ] {
        assert!(
            stdout.contains(expected),
            "skill help missing description {expected:?}: {stdout}"
        );
    }
}

#[test]
fn skill_orphan_help_describes_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "orphan", "--help"])
        .output()
        .expect("run loom skill orphan --help");

    assert!(
        output.status.success(),
        "skill orphan help unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("List orphaned projection records"),
        "skill orphan help missing list description: {stdout}"
    );
    assert!(
        stdout.contains("Remove orphaned projection records (and optionally their live files)"),
        "skill orphan help missing clean description: {stdout}"
    );
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

#[test]
fn risky_command_help_describes_selectors_and_repair_strategy() {
    for (args, expected) in [
        (
            vec!["skill", "capture", "--help"],
            vec![
                "Registry skill name",
                "Binding id",
                "Projection instance id",
                "Git commit message",
            ],
        ),
        (
            vec!["skill", "rollback", "--help"],
            vec![
                "Git revision or snapshot reference",
                "Number of source commits",
                "Preview rollback impact",
            ],
        ),
        (
            vec!["ops", "history", "repair", "--help"],
            vec!["Which side should win"],
        ),
        (vec!["panel", "--help"], vec!["Local HTTP port"]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_loom"))
            .args(args)
            .output()
            .expect("run loom help");
        assert!(
            output.status.success(),
            "help failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        for phrase in expected {
            assert!(stdout.contains(phrase), "help missing {phrase:?}: {stdout}");
        }
    }
}
