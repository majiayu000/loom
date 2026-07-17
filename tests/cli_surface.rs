use std::fs;
use std::net::TcpListener;
use std::process::Command;

mod common;

use common::{
    TestDir, run_loom, run_loom_with_env, write_file, write_minimal_registry_state, write_skill,
};

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
        "Plan and apply guarded MCP server provisioning",
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
    let contract = concat!(
        include_str!("../docs/LOOM_CLI_CONTRACT.md"),
        include_str!("../docs/LOOM_CLI_CONTRACT_OPERATIONS.md")
    );
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
        "INIT_ERROR",
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
        "COMMIT_DIRECTION_AMBIGUOUS",
        "AUDIT_ERROR",
        "LOCK_BUSY",
        "REMOTE_UNREACHABLE",
        "REMOTE_DIVERGED",
        "PUSH_REJECTED",
        "REPLAY_CONFLICT",
        "QUEUE_BLOCKED",
        "ADAPTER_INVALID",
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
        "loom plan converge",
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
        "loom skill author draft",
        "loom skill author extract",
        "loom skill author rewrite",
        "loom skill author tune-description",
        "loom skill author generate-evals",
        "loom skill author apply-patch",
        "loom skill history",
        "loom skill trash add",
        "loom skill trash list",
        "loom skill trash restore",
        "loom skill trash purge",
        "loom skill provenance inspect",
        "loom skill provenance verify",
        "loom skill provenance outdated",
        "loom skill provenance refresh",
        "loom provider add",
        "loom catalog preview",
        "loom package plan",
        "loom package build",
        "loom package verify",
        "loom mcp requirement list",
        "loom mcp plan",
        "loom mcp apply",
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
        "skill provenance outdated",
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
        "mcp apply <plan-id|plan-artifact>",
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
    assert_eq!(skill.len(), 38, "skill command budget changed");
    for removed in [
        "show",
        "capture",
        "save",
        "snapshot",
        "verify",
        "draft",
        "extract",
        "rewrite",
        "tune-description",
        "generate-evals",
        "apply-patch",
        "new",
    ] {
        assert!(
            !skill.contains(&removed.to_string()),
            "removed skill command still present: {removed}"
        );
    }
    assert!(skill.contains(&"author".to_string()));

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skill", "author", "--help"])
        .output()
        .expect("run loom skill author help");
    assert!(output.status.success(), "skill author help should pass");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let author = command_names_from_help(&stdout);
    assert_eq!(
        author,
        [
            "draft",
            "extract",
            "rewrite",
            "tune-description",
            "generate-evals",
            "apply-patch",
            "new",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );

    for removed in [
        "draft",
        "extract",
        "rewrite",
        "tune-description",
        "generate-evals",
        "apply-patch",
        "new",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_loom"))
            .args(["skill", removed, "--help"])
            .output()
            .expect("run removed authoring path");
        assert_eq!(
            output.status.code(),
            Some(2),
            "old path must fail: {removed}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"),
            "old path must return a clap unknown-command error: {removed}"
        );
    }
}

#[test]
fn skillset_help_describes_lifecycle_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .args(["skillset", "--help"])
        .output()
        .expect("run loom skillset help");
    assert!(output.status.success(), "skillset help should pass");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let skillset = command_names_from_help(&stdout);
    for command in [
        "create",
        "add",
        "remove",
        "show",
        "lint",
        "activate",
        "deactivate",
        "eval",
        "release",
        "rollback",
    ] {
        assert!(
            skillset.contains(&command.to_string()),
            "skillset help missing subcommand {command}: {stdout}"
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
fn use_gemini_cli_uses_native_managed_root_to_avoid_codex_collision() {
    let root = TestDir::new("cli-use-gemini-adapter-root");
    let workspace = TestDir::new("cli-use-gemini-workspace");
    let fake_home = TestDir::new("cli-use-gemini-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing Gemini adapter target roots.\n---\n# Demo\n",
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
            "gemini-cli",
            "--workspace",
            &workspace_str,
        ],
    );

    assert!(output.status.success(), "loom use failed: {env}");
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        workspace
            .path()
            .join(".gemini/skills")
            .display()
            .to_string()
    );
}

#[test]
fn use_gemini_cli_keeps_configured_user_roots_out_of_project_scope() {
    let root = TestDir::new("cli-use-gemini-project-scope-root");
    let workspace = TestDir::new("cli-use-gemini-project-scope-workspace");
    let fake_home = TestDir::new("cli-use-gemini-project-scope-home");
    let configured = root.path().join("gemini-user-root");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing Gemini project scope.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join(".env"),
        &format!("GEMINI_CLI_SKILLS_DIR={}\n", configured.display()),
    );

    let home_str = fake_home.path().display().to_string();
    let workspace_str = workspace.path().display().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &[
            "use",
            "demo",
            "--agents",
            "gemini-cli",
            "--workspace",
            &workspace_str,
        ],
    );
    assert!(output.status.success(), "loom use failed: {env}");
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        workspace
            .path()
            .join(".gemini/skills")
            .display()
            .to_string()
    );
}

#[test]
fn use_gemini_cli_ignores_unofficial_dotenv_skill_roots() {
    let root = TestDir::new("cli-use-gemini-dotenv-root");
    let workspace = TestDir::new("cli-use-gemini-dotenv-workspace");
    let fake_home = TestDir::new("cli-use-gemini-dotenv-home");
    let first = root.path().join("gemini-one");
    let second = root.path().join("gemini-two");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing configured Gemini roots.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join(".env"),
        &format!(
            "GEMINI_CLI_SKILLS_DIR={},{}\n",
            first.display(),
            second.display()
        ),
    );

    let home_str = fake_home.path().display().to_string();
    let workspace_str = workspace.path().display().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &[
            "use",
            "demo",
            "--agents",
            "gemini-cli",
            "--scope",
            "user",
            "--workspace",
            &workspace_str,
        ],
    );
    assert!(output.status.success(), "loom use failed: {env}");
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        fake_home
            .path()
            .join(".gemini/skills")
            .display()
            .to_string()
    );
}

#[test]
fn use_gemini_cli_does_not_treat_skills_dir_as_an_official_override() {
    let root = TestDir::new("cli-use-gemini-explicit-official-root");
    let workspace = TestDir::new("cli-use-gemini-explicit-official-workspace");
    let fake_home = TestDir::new("cli-use-gemini-explicit-official-home");
    let agents_root = fake_home.path().join(".agents/skills");
    let second = root.path().join("gemini-two");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing an explicit official Gemini root.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join(".env"),
        &format!(
            "GEMINI_CLI_SKILLS_DIR={},{}\n",
            agents_root.display(),
            second.display()
        ),
    );

    let home_str = fake_home.path().display().to_string();
    let workspace_str = workspace.path().display().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &[
            "use",
            "demo",
            "--agents",
            "gemini-cli",
            "--scope",
            "user",
            "--workspace",
            &workspace_str,
        ],
    );
    assert!(output.status.success(), "loom use failed: {env}");
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        fake_home
            .path()
            .join(".gemini/skills")
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
fn app_init_failure_is_a_structured_json_envelope() {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--request-id")
        .arg("req-init-failure")
        .args(["workspace", "status"])
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .output()
        .expect("run loom without a home directory");

    assert_eq!(output.status.code(), Some(3));
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse app init failure envelope");
    assert_eq!(env["ok"], serde_json::json!(false));
    assert_eq!(env["cmd"], serde_json::json!("app.init"));
    assert_eq!(env["request_id"], serde_json::json!("req-init-failure"));
    assert_eq!(env["error"]["code"], serde_json::json!("INIT_ERROR"));
    assert_eq!(
        env["error"]["details"]["stage"],
        serde_json::json!("app.init")
    );
}

#[test]
fn panel_bind_failure_is_a_structured_json_envelope() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve panel port");
    let port = listener.local_addr().expect("reserved address").port();
    let root = TestDir::new("cli-panel-bind-failure");
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--request-id")
        .arg("req-panel-failure")
        .arg("--root")
        .arg(root.path())
        .args(["panel", "--port", &port.to_string()])
        .output()
        .expect("run panel on occupied port");

    assert_eq!(output.status.code(), Some(5));
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse panel failure envelope");
    assert_eq!(env["ok"], serde_json::json!(false));
    assert_eq!(env["cmd"], serde_json::json!("panel"));
    assert_eq!(env["request_id"], serde_json::json!("req-panel-failure"));
    assert_eq!(env["error"]["code"], serde_json::json!("IO_ERROR"));
    assert_eq!(
        env["error"]["details"]["stage"],
        serde_json::json!("panel.serve")
    );
    assert_eq!(env["error"]["details"]["port"], serde_json::json!(port));
    assert!(
        output.stderr.is_empty(),
        "JSON panel startup failure must not emit human stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn option_like_skill_name_is_routable_after_argument_separator() {
    let root = TestDir::new("cli-option-like-skill");
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root.path())
        .args(["skill", "inspect", "--", "-demo"])
        .output()
        .expect("inspect option-like skill name");

    assert_eq!(output.status.code(), Some(3));
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse skill failure envelope");
    assert_eq!(env["cmd"], serde_json::json!("skill.inspect"));
    assert_eq!(env["error"]["code"], serde_json::json!("SKILL_NOT_FOUND"));
    assert!(
        env["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("'-demo'")),
        "separator must preserve the option-like positional: {env}"
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

#[path = "cli_surface/command_tests.rs"]
mod command_tests;
