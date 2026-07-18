use super::*;

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
        "Import a skill source",
        "List registry and observed skills",
        "Inspect skill lifecycle status (read-only)",
        "Search and explain skills",
        "Recommend skills for a task (read-only)",
        "Resolve a skill for a task (read-only)",
        "Manage guarded skill authoring artifacts",
        "Project a skill into a bound target",
        "Commit source or live-projection changes",
        "Tag a skill release",
        "Roll back a skill source",
        "Diff skill source revisions",
        "Run offline trigger, task, and artifact evals",
        "Manage compiled runtime artifacts",
        "Continuously import observed skills",
        "Import observed skills once",
        "Inspect and clean orphan projections",
    ] {
        assert!(
            stdout.contains(expected),
            "skill help missing description {expected:?}: {stdout}"
        );
    }

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "author", "--help"])
        .output()
        .expect("run loom skill author --help");
    assert!(output.status.success(), "skill author help should pass");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "Draft a new skill as a guarded patch artifact",
        "Extract reviewed diff context into a guarded patch artifact",
        "Rewrite one skill as a guarded patch artifact",
        "Tune one skill description as a guarded patch artifact",
        "Generate reviewable eval fixture diffs as a patch artifact",
        "Apply a reviewed skill patch artifact through validation gates",
        "Create a lint-clean local skill skeleton",
    ] {
        assert!(
            stdout.contains(expected),
            "skill author help missing description {expected:?}: {stdout}"
        );
    }
}

#[test]
fn skill_compile_help_describes_read_only_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "compile", "--help"])
        .output()
        .expect("run loom skill compile --help");

    assert!(
        output.status.success(),
        "skill compile help unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "--dry-run",
        "--agent",
        "--profile",
        "--skill",
        "List known compiled artifacts for one skill without mutation",
        "Verify compiled artifact manifests, sidecars, digests, and gates",
    ] {
        assert!(
            stdout.contains(expected),
            "skill compile help missing {expected:?}: {stdout}"
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
            vec!["skill", "commit", "--help"],
            vec![
                "Registry skill name",
                "Binding id",
                "Projection instance id",
                "Git commit message",
                "--from-projection",
                "--from-source",
            ],
        ),
        (
            vec!["skill", "rollback", "--help"],
            vec![
                "Git revision or snapshot reference",
                "Number of source commits",
                "--dry-run",
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
