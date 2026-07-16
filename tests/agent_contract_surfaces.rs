use serde_json::json;

mod common;

use common::{TestDir, run_loom, write_file};
use skillloom::cli_contract::{
    CLI_CONTRACT_VERSION, PublicArgvErrorKind, check_surface_inventory, contract_version_matches,
    current_contract_version, parse_contract_version, validate_public_argv,
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
    let report = check_surface_inventory(std::path::Path::new(".")).expect("surface inventory");
    assert!(report.surface_count >= 6);
    assert!(report.example_count >= report.surface_count);
    assert!(report.command_count > 100);
}

#[test]
fn unclassified_command_fails() {
    let root = TestDir::new("unclassified-contract-command");
    write_file(&root.path().join("README.md"), "loom skill list\n");
    write_file(
        &root.path().join("docs/agent-command-surfaces.toml"),
        r#"[[surface]]
id = "readme"
path = "README.md"

[[example]]
id = "readme.command"
surface = "readme"
start_line = 1
end_line = 1
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
        r#"[[surface]]
id = "readme"
path = "README.md"

[[example]]
id = "readme.command"
surface = "readme"
start_line = 1
end_line = 1
classification = "executable"
"#,
    );
    let error = check_surface_inventory(root.path()).expect_err("removed command must fail");
    assert!(error.to_string().contains("README.md:1"));
    assert!(error.to_string().contains("readme.command"));
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
fn hidden_flags_fail() {
    let error = validate_public_argv(["loom", "skill", "watch", "demo", "--max-cycles", "1"])
        .expect_err("hidden flag must fail");
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
fn removed_commands_fail() {
    let error = validate_public_argv(["loom", "skill", "save", "demo"])
        .expect_err("removed command must fail");
    assert_eq!(error.kind, PublicArgvErrorKind::Parse);
}

#[test]
fn contract_additive_capability_requires_minor_bump() {
    let history =
        std::fs::read_to_string("docs/cli-contract-history.toml").expect("read contract history");
    assert!(history.contains("version = \"1.0.0\""));
    assert!(history.contains("migration_note ="));
}
