use serde_json::json;

mod common;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};
use skillloom::cli_contract::{
    CLI_CONTRACT_VERSION, PublicArgvErrorKind, check_contract_range_policy,
    check_next_action_trace, check_surface_inventory, contract_version_matches,
    current_contract_version, load_surface_inventory, parse_contract_version, validate_public_argv,
};

#[test]
fn cli_contract_semver_is_exposed_and_declared() {
    assert_eq!(current_contract_version().major, 1);
    assert!(parse_contract_version("").is_err());
    assert!(parse_contract_version("1.0").is_err());
    assert!(parse_contract_version("01.0.0").is_err());

    let root = TestDir::new("cli-contract-version");
    let (output, envelope) = run_loom(root.path(), &["workspace", "status"]);
    assert!(output.status.success(), "status failed: {envelope}");
    assert_eq!(
        envelope["cli_contract_version"],
        json!(CLI_CONTRACT_VERSION)
    );

    let metadata = std::fs::read_to_string("skills/loom-registry/loom.skill.toml")
        .expect("read shipped Skill metadata");
    assert!(metadata.contains("cli_contract = \">=1.0.0,<2.0.0\""));
}

#[test]
fn incompatible_cli_blocks_mutation() {
    assert!(contract_version_matches(">=1.0.0,<2.0.0", "1.0.0").unwrap());
    assert!(!contract_version_matches(">=1.0.0,<2.0.0", "2.0.0").unwrap());
    assert!(contract_version_matches(">=1.0.0,<2.0.0", "").is_err());
    assert!(contract_version_matches("", "1.0.0").is_err());
}

#[test]
fn new_skill_old_cli_blocks_mutation() {
    assert!(!contract_version_matches(">=1.1.0,<2.0.0", "1.0.0").unwrap());
}

#[test]
fn executable_examples_parse() {
    let parsed = validate_public_argv([
        "loom",
        "--json",
        "--root",
        "/tmp/registry",
        "skill",
        "inspect",
        "demo",
        "--agent",
        "codex",
    ])
    .expect("public argv");
    assert_eq!(parsed.command_path, ["loom", "skill", "inspect"]);
    assert!(
        parsed
            .explicit_args
            .iter()
            .any(|argument| argument == "agent")
    );
}

#[test]
fn inventory_covers_public_surfaces() {
    let inventory = load_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    assert!(
        inventory
            .agent_capabilities
            .iter()
            .any(|capability| capability == "field:envelope.cli_contract_version:semver-string")
    );
    let report = check_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    assert!(report.surface_count >= 6);
    assert!(report.example_count >= report.surface_count);
    assert!(report.command_count > 100);
    assert_eq!(report.next_action_emitter_count, 57);
    assert_eq!(report.panel_mutation_count, 25);
}

#[test]
fn emitter_inventory_is_complete() {
    let report = check_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    assert_eq!(report.next_action_emitter_count, 57);
}

#[test]
fn emitter_fixture_identity_is_observable() {
    let root = TestDir::new("emitter-identity");
    let home = TestDir::new("emitter-identity-home");
    let trace = root.path().join("next-actions.jsonl");
    write_file(&trace, "");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking emitter identity.\n---\n# Demo\n",
    );
    let trace_arg = trace.to_string_lossy().into_owned();
    let home_arg = home.path().to_string_lossy().into_owned();
    let envs = [
        ("LOOM_TEST_NEXT_ACTION_TRACE", trace_arg.as_str()),
        ("HOME", home_arg.as_str()),
    ];
    let observed_path = home.path().join("observed/skills");
    std::fs::create_dir_all(&observed_path).expect("create observed target");
    let observed_arg = observed_path.to_string_lossy().into_owned();
    let (output, target) = run_loom_with_env(
        root.path(),
        &envs,
        &[
            "target",
            "add",
            "--agent",
            "codex",
            "--path",
            &observed_arg,
            "--ownership",
            "observed",
        ],
    );
    assert!(output.status.success(), "target add should pass: {target}");
    let target_id = target["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    let (output, missing) =
        run_loom_with_env(root.path(), &envs, &["target", "show", "missing-target"]);
    assert!(
        !output.status.success(),
        "missing target should fail: {missing}"
    );
    let (output, unmanaged) = run_loom_with_env(
        root.path(),
        &envs,
        &[
            "skill", "activate", "demo", "--agent", "codex", "--target", target_id,
        ],
    );
    assert!(
        !output.status.success(),
        "observed target activation should fail: {unmanaged}"
    );

    let records = std::fs::read_to_string(&trace)
        .expect("read trace")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("trace JSON"))
        .collect::<Vec<_>>();
    let payload = |emitter_id: &str| {
        records
            .iter()
            .find(|record| record["emitter_id"] == emitter_id)
            .unwrap_or_else(|| panic!("missing trace for {emitter_id}: {records:?}"))["payload"]
            .clone()
    };
    let not_found = payload("error.target_not_found");
    let not_managed = payload("error.target_not_managed");
    assert_eq!(not_found[0]["cmd"], json!("loom target list --json"));
    assert_eq!(not_managed[0]["cmd"], json!("loom target list --json"));
    assert_ne!(
        records
            .iter()
            .position(|record| record["emitter_id"] == "error.target_not_found"),
        records
            .iter()
            .position(|record| record["emitter_id"] == "error.target_not_managed")
    );
}

#[test]
fn emitter_trace_payloads_parse() {
    let fallback = TestDir::new("emitter-trace-payloads");
    let path = if let Some(path) = std::env::var_os("LOOM_CONTRACT_TRACE_INPUT") {
        std::path::PathBuf::from(path)
    } else {
        let path = fallback.path().join("next-actions.jsonl");
        write_file(
            &path,
            r#"{"emitter_id":"error.target_not_found","fixture_id":"error_actions::tests::default_next_actions_cover_top_guidance_errors","payload_type":"alloc::vec::Vec<loom::error_actions::NextAction>","payload":[{"cmd":"loom target list --json","reason":"inspect targets"}]}
"#,
        );
        path
    };
    let inventory = load_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    let emitters = if std::env::var_os("LOOM_CONTRACT_TRACE_INPUT").is_some() {
        inventory.next_action_emitters
    } else {
        inventory
            .next_action_emitters
            .into_iter()
            .filter(|emitter| emitter.id == "error.target_not_found")
            .collect()
    };
    let report = check_next_action_trace(&path, &emitters).expect("next-action trace payloads");
    assert!(report.record_count >= 1);
    assert!(report.command_count >= 1);
    if let Ok(expected) = std::env::var("LOOM_CONTRACT_TRACE_EXPECTED_EMITTERS") {
        assert_eq!(
            report.emitter_count,
            expected.parse::<usize>().expect("expected emitter count")
        );
    }
}

#[test]
fn emitter_trace_rejects_shape_and_fixture_drift() {
    let root = TestDir::new("emitter-trace-contract-drift");
    let inventory = load_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    let emitters = inventory
        .next_action_emitters
        .iter()
        .filter(|emitter| emitter.id == "error.target_not_found")
        .cloned()
        .collect::<Vec<_>>();
    let trace = root.path().join("wrong-shape.jsonl");
    write_file(
        &trace,
        r#"{"emitter_id":"error.target_not_found","fixture_id":"error_actions::tests::default_next_actions_cover_top_guidance_errors","payload_type":"alloc::vec::Vec<alloc::string::String>","payload":["loom target list --json"]}
"#,
    );
    let error = check_next_action_trace(&trace, &emitters).expect_err("shape drift must fail");
    assert!(error.to_string().contains("Object shape"), "{error}");

    write_file(
        &trace,
        r#"{"emitter_id":"error.target_not_found","fixture_id":"invented.fixture","payload_type":"alloc::vec::Vec<loom::error_actions::NextAction>","payload":[{"cmd":"loom target list --json"}]}
"#,
    );
    let error = check_next_action_trace(&trace, &emitters).expect_err("fixture drift must fail");
    assert!(
        error
            .to_string()
            .contains("did not produce declared fixture"),
        "{error}"
    );
}

#[test]
fn empty_trace_shape_requires_exact_type_identity() {
    let root = TestDir::new("emitter-empty-shape-identity");
    let inventory = load_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    let emitters = inventory
        .next_action_emitters
        .iter()
        .filter(|emitter| emitter.id == "error.target_not_found")
        .cloned()
        .collect::<Vec<_>>();
    let trace = root.path().join("deceptive-empty-shape.jsonl");
    write_file(
        &trace,
        r#"{"emitter_id":"error.target_not_found","fixture_id":"error_actions::tests::default_next_actions_cover_top_guidance_errors","payload_type":"alloc::vec::Vec<NextActionText>","payload":[]}
"#,
    );
    let error = check_next_action_trace(&trace, &emitters)
        .expect_err("a substring lookalike must not prove empty object shape");
    assert!(error.to_string().contains("no payload proving"), "{error}");

    let emitters = inventory
        .next_action_emitters
        .iter()
        .filter(|emitter| emitter.id == "codex.visibility.report")
        .cloned()
        .collect::<Vec<_>>();
    write_file(
        &trace,
        r#"{"emitter_id":"codex.visibility.report","fixture_id":"compiled_activation_materializes_valid_artifact_projection","payload_type":"alloc::vec::Vec<StringAction>","payload":[]}
"#,
    );
    let error = check_next_action_trace(&trace, &emitters)
        .expect_err("a substring lookalike must not prove empty string shape");
    assert!(error.to_string().contains("no payload proving"), "{error}");
}

#[test]
fn active_command_guidance_is_parser_checked() {
    let inventory = load_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    let classification = inventory
        .examples
        .iter()
        .find(|example| example.id == "readme.command_matrix")
        .map(|example| example.classification);
    assert_eq!(
        classification,
        Some(skillloom::cli_contract::ExampleClassification::CommandReference)
    );
    let error = validate_public_argv(["loom", "skill", "save", "--help"])
        .expect_err("removed command reference must fail");
    assert_eq!(error.kind, PublicArgvErrorKind::Parse);
}

#[test]
fn library_facade_does_not_export_raw_vocabulary() {
    let source = std::fs::read_to_string("src/lib.rs").expect("read library facade");
    assert!(!source.contains("pub mod vocab;"));
    assert!(!source.contains("pub mod cli;"));
}

#[test]
fn panel_cli_equivalents_parse() {
    let report = check_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    assert_eq!(report.panel_mutation_count, 25);
}

#[test]
fn panel_mutations_are_mapped() {
    let report = check_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    assert_eq!(report.panel_mutation_count, 25);
}

#[test]
fn unclassified_command_fails() {
    let root = TestDir::new("unclassified-contract-command");
    write_file(&root.path().join("README.md"), "loom skill list\n");
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        r#"agent_capabilities = ["field:fixture.ok:boolean"]
[[surface]]
id = "readme"
path = "README.md"

[[example]]
id = "readme.command"
surface = "readme"
line_range = [1, 1]
classification = "unknown"
"#,
    );
    let error = check_surface_inventory(root.path()).expect_err("classification must fail");
    assert!(error.to_string().contains("closed classification set"));
}

#[test]
fn parse_failure_is_terminal() {
    let root = TestDir::new("invalid-contract-command");
    write_file(&root.path().join("README.md"), "loom skill save demo\n");
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        r#"agent_capabilities = ["field:fixture.ok:boolean"]
[[surface]]
id = "readme"
path = "README.md"

[[example]]
id = "readme.command"
surface = "readme"
line_range = [1, 1]
classification = "executable"
[[next_action_emitter]]
id = "fixture.emitter"
source = "src/fixture.rs#next_actions"
shape = "string"
fixture_ids = ["fixture.emitter.output"]
[[panel_mutation]]
id = "panel.fixture"
label_path = "panel/src/lib/operation_labels.ts"
action_id = "fixture.write"
backend_route = "POST /api/v1/write"
handler = "write"
binding = "cli_equivalent"
cli_argv = ["loom", "workspace", "status"]
"#,
    );
    write_minimal_panel_contract(root.path());
    write_file(
        &root.path().join("src/fixture.rs"),
        "fn fixture() { observe_next_actions(\"fixture.emitter\", Vec::<String>::new()); }\n",
    );
    let error = check_surface_inventory(root.path()).expect_err("removed command must fail");
    assert!(error.to_string().contains("README.md:1"), "{error}");
    assert!(error.to_string().contains("readme.command"), "{error}");
}

fn write_minimal_panel_contract(root: &std::path::Path) {
    write_file(&root.join("skills/.contract-scan-root"), "");
    write_file(&root.join(".github/workflows/.contract-scan-root"), "");
    write_file(
        &root.join("src/panel/mod.rs"),
        "Router::new().route(\"/api/v1/write\", post(write))\n",
    );
    write_file(
        &root.join("src/panel/handlers/write.rs"),
        r#"fn write() {
    ensure_mutation_authorized(&state, peer, &headers, "fixture.write");
}
"#,
    );
    write_file(
        &root.join("panel/src/lib/api/client.ts"),
        "export const api = {\n  write: () => postJson(\"/api/v1/write\", {}),\n}\n",
    );
    write_file(
        &root.join("panel/src/lib/operation_labels.ts"),
        "const ACTION_LABELS = { \"fixture.write\": \"Write\" };\n",
    );
}

#[test]
fn checker_is_read_only_and_repeatable() {
    let before = std::fs::read("docs/agent-command-surfaces.toml").expect("inventory bytes");
    let first = check_surface_inventory(std::path::Path::new(".")).expect("first check");
    let second = check_surface_inventory(std::path::Path::new(".")).expect("second check");
    let after = std::fs::read("docs/agent-command-surfaces.toml").expect("inventory bytes");
    assert_eq!(first, second);
    assert_eq!(before, after);
}

#[test]
fn checker_never_rewrites_sources() {
    let before = std::fs::read("docs/agent-command-surfaces.toml").expect("inventory bytes");
    check_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    let after = std::fs::read("docs/agent-command-surfaces.toml").expect("inventory bytes");
    assert_eq!(before, after);
}

#[test]
fn hidden_flags_fail() {
    let error = validate_public_argv(["loom", "skill", "watch", "demo", "--max-cycles", "1"])
        .expect_err("hidden flag must fail");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);
}

#[test]
fn hidden_flags_with_help_fail() {
    let error = validate_public_argv([
        "loom",
        "skill",
        "watch",
        "demo",
        "--max-cycles",
        "1",
        "--help",
    ])
    .expect_err("hidden flag must not be exposed through help");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenArgument);
}

#[test]
fn hidden_commands_fail() {
    let error = validate_public_argv([
        "loom",
        "workflow",
        "run",
        "workflow-plan",
        "--agent",
        "codex",
        "--workspace",
        "/tmp/workspace",
    ])
    .expect_err("hidden command must fail");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenCommand);
}

#[test]
fn hidden_commands_with_help_fail() {
    let error = validate_public_argv(["loom", "workflow", "run", "--help"])
        .expect_err("hidden command must not be exposed through help");
    assert_eq!(error.kind, PublicArgvErrorKind::HiddenCommand);
}

#[test]
fn nested_public_surface_requires_inventory() {
    let root = TestDir::new("nested-public-contract-surface");
    write_file(&root.path().join("README.md"), "loom workspace status\n");
    write_file(
        &root.path().join("docs/plan/stale.md"),
        "legacy prose says `loom skill save demo`\n",
    );
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        r#"agent_capabilities = ["field:fixture.ok:boolean"]
[[surface]]
id = "readme"
path = "README.md"

[[example]]
id = "readme.status"
surface = "readme"
line_range = [1, 1]
classification = "executable"
[[next_action_emitter]]
id = "fixture.emitter"
source = "src/fixture.rs#next_actions"
shape = "string"
fixture_ids = ["fixture.emitter.output"]
[[panel_mutation]]
id = "panel.fixture"
label_path = "panel/src/lib/operation_labels.ts"
action_id = "fixture.write"
backend_route = "POST /api/v1/write"
handler = "write"
binding = "cli_equivalent"
cli_argv = ["loom", "workspace", "status"]
"#,
    );
    write_minimal_panel_contract(root.path());
    write_file(
        &root.path().join("src/fixture.rs"),
        "fn fixture() { observe_next_actions(\"fixture.emitter\", Vec::<String>::new()); }\n",
    );
    let error = check_surface_inventory(root.path()).expect_err("nested surface must be covered");
    assert!(error.to_string().contains("docs/plan/stale.md"), "{error}");
}

#[test]
fn whole_file_classification_fails() {
    let root = TestDir::new("whole-file-contract-classification");
    write_file(
        &root.path().join("README.md"),
        "loom workspace status\nexplanatory prose\n",
    );
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        r#"agent_capabilities = ["field:fixture.ok:boolean"]
[[surface]]
id = "readme"
path = "README.md"

[[example]]
id = "readme.whole_file"
surface = "readme"
line_range = [1, 2]
classification = "executable"
[[next_action_emitter]]
id = "fixture.emitter"
source = "src/fixture.rs#next_actions"
shape = "string"
fixture_ids = ["fixture.emitter.output"]
[[panel_mutation]]
id = "panel.fixture"
label_path = "panel/src/lib/operation_labels.ts"
action_id = "fixture.write"
backend_route = "POST /api/v1/write"
handler = "write"
binding = "cli_equivalent"
cli_argv = ["loom", "workspace", "status"]
"#,
    );
    write_minimal_panel_contract(root.path());
    write_file(
        &root.path().join("src/fixture.rs"),
        "fn fixture() { observe_next_actions(\"fixture.emitter\", Vec::<String>::new()); }\n",
    );
    let error = check_surface_inventory(root.path()).expect_err("whole-file range must fail");
    assert!(error.to_string().contains("whole-file"), "{error}");
}

#[test]
fn removed_commands_fail() {
    let error = validate_public_argv(["loom", "skill", "save", "demo"])
        .expect_err("removed command must fail");
    assert_eq!(error.kind, PublicArgvErrorKind::Parse);
}

#[test]
fn contract_additive_capability_requires_minor_bump() {
    let root = TestDir::new("contract-capability-minor-policy");
    write_file(
        &root.path().join("skills/loom-registry/loom.skill.toml"),
        "[compatibility]\ncli_contract = \">=1.0.0,<2.0.0\"\n",
    );
    write_file(
        &root.path().join("docs/cli-contract-history.toml"),
        "[[contract]]\nversion = \"1.0.0\"\nskill_range = \">=1.0.0,<2.0.0\"\nmigration_note = \"bootstrap\"\n",
    );
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        r#"agent_capabilities = ["field:fixture.ok:boolean"]
[[surface]]
id = "readme"
path = "README.md"
[[example]]
id = "readme.status"
surface = "readme"
line_range = [1, 1]
classification = "executable"
[[next_action_emitter]]
id = "fixture.emitter"
source = "src/fixture.rs#next_actions"
shape = "string"
fixture_ids = ["fixture.emitter.output"]
[[panel_mutation]]
id = "panel.fixture"
label_path = "panel/labels.ts"
action_id = "fixture.write"
backend_route = "POST /api/v1/write"
handler = "write"
binding = "cli_equivalent"
cli_argv = ["loom", "workspace", "status"]
"#,
    );
    write_file(&root.path().join("README.md"), "loom workspace status\n");
    write_file(
        &root.path().join("src/cli_contract.rs"),
        "pub const CLI_CONTRACT_VERSION: &str = \"1.0.0\";\n",
    );
    write_file(
        &root.path().join("CHANGELOG.md"),
        "CLI contract bootstrap\n",
    );
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(root.path())
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git UTF-8")
            .trim()
            .to_string()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "contract@example.invalid"]);
    git(&["config", "user.name", "Contract Fixture"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let base = git(&["rev-parse", "HEAD"]);

    write_file(
        &root.path().join("README.md"),
        "loom workspace status\nloom workspace doctor\n",
    );
    let inventory = std::fs::read_to_string(root.path().join("docs/agent-command-surfaces.toml"))
        .expect("read fixture inventory");
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        &(inventory
            + "[[example]]\nid = \"readme.doctor\"\nsurface = \"readme\"\nline_range = [2, 2]\nclassification = \"executable\"\n"),
    );
    let error = check_contract_range_policy(root.path(), Some(&base))
        .expect_err("additive capability without minor bump must fail");
    assert!(error.to_string().contains("minor bump"), "{error}");

    write_file(
        &root.path().join("src/cli_contract.rs"),
        "pub const CLI_CONTRACT_VERSION: &str = \"1.1.0\";\n",
    );
    let history = std::fs::read_to_string(root.path().join("docs/cli-contract-history.toml"))
        .expect("read fixture history");
    write_file(
        &root.path().join("docs/cli-contract-history.toml"),
        &(history
            + "[[contract]]\nversion = \"1.1.0\"\nskill_range = \">=1.0.0,<2.0.0\"\nmigration_note = \"add doctor\"\n"),
    );
    check_contract_range_policy(root.path(), Some(&base))
        .expect("minor bump must admit additive capability");
}

#[test]
fn contract_range_requires_migration_note_with_explicit_base() {
    let base = std::env::var("LOOM_CONTRACT_DIFF_BASE").unwrap_or_else(|_| "HEAD~1".to_string());
    check_contract_range_policy(std::path::Path::new("."), Some(&base))
        .expect("explicit-base contract policy");
}

#[test]
fn contract_range_missing_diff_base_fails() {
    let error = check_contract_range_policy(std::path::Path::new("."), None)
        .expect_err("missing diff base must fail closed");
    assert!(error.to_string().contains("LOOM_CONTRACT_DIFF_BASE"));
}

#[test]
fn contract_range_policy_current_diff() {
    let base = std::env::var("LOOM_CONTRACT_DIFF_BASE").unwrap_or_else(|_| "HEAD~1".to_string());
    check_contract_range_policy(std::path::Path::new("."), Some(&base))
        .expect("current contract policy");
}
