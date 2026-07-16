use serde_json::json;

mod common;

use common::{TestDir, run_loom};
use skillloom::cli_contract::{
    CLI_CONTRACT_VERSION, PublicArgvErrorKind, contract_version_matches, current_contract_version,
    parse_contract_version, validate_public_argv,
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
