use std::fs;
use std::process::Command;

mod common;

use common::{TestDir, run_loom, run_loom_with_env, write_minimal_registry_state, write_skill};

fn command_names_from_help(stdout: &str) -> Vec<String> {
    let mut in_commands = false;
    let mut names = Vec::new();
    for line in stdout.lines() {
        match line {
            "Commands:" => {
                in_commands = true;
                continue;
            }
            "Options:" => break,
            _ => {}
        }
        if !in_commands {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(name) = trimmed.split_whitespace().next()
            && name != "help"
        {
            names.push(name.to_string());
        }
    }
    names
}

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
        "Plan or apply a human-friendly skill use flow",
        "Create durable, audited agent plans",
        "Apply a durable agent plan with an idempotency key",
        "Inspect and configure registry workspace state",
        "Register and inspect agent skill directories",
        "Manage skill sources, projections, and versions",
        "Manage groups of registry skills",
        "Manage local privacy-preserving telemetry and analytics",
        "Manage skill catalog providers",
        "Search and preview skill catalogs",
        "Plan, build, and verify portable skill packages",
        "Plan MCP server requirements and provisioning without mutation",
        "Plan remote and devcontainer skill provisioning without mutation",
        "Inspect non-skill instruction surfaces without mutation",
        "Plan and preflight guarded multi-skill workflows",
        "Synchronize the registry through its Git remote",
        "Inspect, replay, and repair operation history",
        "Inspect and reconcile Codex active-view visibility",
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
        "`use`",
        "`plan`",
        "`apply`",
        "`workspace`",
        "`target`",
        "`skill`",
        "`skillset`",
        "`telemetry`",
        "`provider`",
        "`catalog`",
        "`package`",
        "`mcp`",
        "`provision`",
        "`instruction`",
        "`workflow`",
        "`sync`",
        "`ops`",
        "`agent`",
        "`codex`",
        "`panel`",
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
        "STATE_NOT_INITIALIZED",
        "PROVIDER_NOT_FOUND",
        "SKILL_NOT_FOUND",
        "BINDING_NOT_FOUND",
        "TARGET_NOT_FOUND",
        "TRASH_ENTRY_NOT_FOUND",
        "TARGET_NOT_MANAGED",
        "TARGET_AGENT_MISMATCH",
        "PROJECTION_CONFLICT",
        "PROJECTION_METHOD_UNSUPPORTED",
        "POLICY_BLOCKED",
        "EVAL_FAILED",
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
        "loom workspace doctor",
        "loom use",
        "loom plan use",
        "loom apply",
        "loom skill list",
        "loom skill inspect",
        "loom skill inspect --brief",
        "loom skill deps",
        "loom skill compile",
        "loom skill activate",
        "loom skill deactivate",
        "loom skill active list",
        "loom skill visibility",
        "loom skill search",
        "loom skill search",
        "loom skill draft",
        "loom skill extract",
        "loom skill rewrite",
        "loom skill tune-description",
        "loom skill generate-evals",
        "loom skill apply-patch",
        "loom skill history",
        "loom skill trash add",
        "loom skill trash list",
        "loom skill trash restore",
        "loom skill trash purge",
        "loom skill provenance inspect",
        "loom skill provenance verify",
        "loom skill provenance refresh",
        "loom provider add",
        "loom catalog preview",
        "loom package plan",
        "loom package build",
        "loom package verify",
        "loom mcp requirement list",
        "loom mcp plan",
        "loom mcp doctor",
        "loom mcp catalog search",
        "loom mcp catalog show",
        "loom provision plan",
        "loom provision doctor",
        "loom provision apply",
        "loom provision export",
        "loom provision import",
        "loom instruction scan",
        "loom instruction show",
        "loom instruction classify",
        "loom instruction doctor",
        "loom instruction migrate-plan",
        "loom skill install",
        "loom skill policy",
        "loom skill eval",
        "loom workflow plan",
        "loom workflow preflight",
        "loom codex reconcile",
        "loom skill watch",
        "loom telemetry status",
        "loom telemetry enable",
        "loom telemetry report",
        "loom telemetry export",
        "loom telemetry purge",
    ] {
        assert!(
            readme.contains(command),
            "README CLI reference missing command {command}"
        );
    }

    for command in [
        "skill add <path|git-url|github:owner/repo//subdir>",
        "skill inspect",
        "skill deps <skill-id>",
        "skill compile <skill-id> --dry-run",
        "skill activate",
        "skill deactivate",
        "skill active list",
        "skill visibility",
        "codex reconcile",
        "skill provenance inspect",
        "skill provenance verify",
        "skill provenance refresh",
        "loom.lock",
        "skill policy <skill-id>",
        "skill eval <skill-id>",
        "POLICY_BLOCKED",
        "workflow plan <workflow-id>",
        "workflow preflight <plan-id>",
        "package plan <skill:<skill>|skillset:<skillset>>",
        "package verify <artifact>",
        "mcp requirement list --skill <skill>",
        "mcp plan --skill <skill>",
        "mcp catalog show <server>",
        "provision plan --target devcontainer",
        "provision doctor --target devcontainer",
        "provision apply <plan-id|plan-artifact>",
        "instruction scan",
        "instruction migrate-plan",
        "telemetry report",
        "telemetry export --format jsonl|csv",
        "telemetry purge",
    ] {
        assert!(
            contract.contains(command),
            "CLI contract missing provenance surface {command}"
        );
    }
}

#[test]
fn command_surface_budget_tracks_read_surface_convergence() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--help")
        .output()
        .expect("run loom help");
    assert!(output.status.success(), "top-level help should pass");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let top_level = command_names_from_help(&stdout);
    assert_eq!(top_level.len(), 28, "top-level command budget changed");
    assert!(!top_level.contains(&"doctor".to_string()));

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "--help"])
        .output()
        .expect("run loom skill help");
    assert!(output.status.success(), "skill help should pass");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let skill = command_names_from_help(&stdout);
    assert_eq!(skill.len(), 40, "skill command budget changed");
    for removed in [
        "show",
        "resolve",
        "recommend",
        "capture",
        "save",
        "snapshot",
        "verify",
    ] {
        assert!(
            !skill.contains(&removed.to_string()),
            "removed skill command still present: {removed}"
        );
    }
}

#[test]
fn use_codex_project_default_comes_from_adapter_metadata() {
    let root = TestDir::new("cli-use-codex-adapter-root");
    let workspace = TestDir::new("cli-use-codex-workspace");
    let fake_home = TestDir::new("cli-use-codex-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing adapter-driven target roots.\n---\n# Demo\n",
    );

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let workspace_str = workspace.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &[
            "use",
            "demo",
            "--agents",
            "codex",
            "--workspace",
            &workspace_str,
        ],
    );

    assert!(
        output.status.success(),
        "loom use failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        workspace
            .path()
            .join(".agents/skills")
            .display()
            .to_string()
    );
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
fn not_found_errors_include_structured_next_actions() {
    let root = TestDir::new("cli-error-next-actions");
    write_minimal_registry_state(root.path(), 1);

    let cases = [
        (
            vec!["skill", "inspect", "missing-skill"],
            "SKILL_NOT_FOUND",
            "loom skill list --json",
        ),
        (
            vec!["workspace", "binding", "show", "missing-binding"],
            "BINDING_NOT_FOUND",
            "loom workspace binding list --json",
        ),
        (
            vec!["target", "show", "missing-target"],
            "TARGET_NOT_FOUND",
            "loom target list --json",
        ),
    ];

    for (args, code, command) in cases {
        let (output, env) = run_loom(root.path(), &args);
        assert!(!output.status.success(), "{env}");
        assert_eq!(env["error"]["code"], serde_json::json!(code));
        assert_eq!(
            env["error"]["next_actions"][0]["cmd"],
            serde_json::json!(command),
            "{env}"
        );
        assert!(
            env["error"]["next_actions"][0]["reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()),
            "{env}"
        );
    }
}

#[test]
fn state_not_initialized_error_includes_next_action() {
    let root = TestDir::new("cli-state-next-action");

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(!output.status.success(), "{env}");
    assert_eq!(
        env["error"]["code"],
        serde_json::json!("STATE_NOT_INITIALIZED")
    );
    assert_eq!(
        env["error"]["next_actions"][0]["cmd"],
        serde_json::json!("loom workspace init --json"),
        "{env}"
    );
}

#[test]
fn human_not_found_errors_print_next_action_hints() {
    let root = TestDir::new("cli-human-error-hint");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--root")
        .arg(root.path())
        .args(["skill", "inspect", "missing-skill"])
        .output()
        .expect("run loom");

    assert!(
        !output.status.success(),
        "missing skill unexpectedly succeeded: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hint: try loom skill list --json"),
        "human error output should include next-action hint: {stderr}"
    );
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
        "List registry and observed skills",
        "Inspect one skill lifecycle status without mutating state",
        "Search, resolve, and explain skills with deterministic scoring",
        "Draft a new skill as a guarded patch artifact",
        "Extract reviewed diff context into a guarded patch artifact",
        "Rewrite one skill as a guarded patch artifact",
        "Tune one skill description as a guarded patch artifact",
        "Generate reviewable eval fixture diffs as a patch artifact",
        "Apply a reviewed skill patch artifact through validation gates",
        "Project a registry skill into a bound target",
        "Commit source changes from the registry or a live projection",
        "Tag a skill release",
        "Roll back a skill source to an earlier revision",
        "Diff two revisions of a skill source",
        "Run offline skill eval fixtures for trigger, task, and artifact checks",
        "Plan, write, list, and verify derived compiled runtime artifacts",
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
