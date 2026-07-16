use serde_json::json;

mod common;

use common::{TestDir, run_loom};
use skillloom::cli_contract::{
    CLI_CONTRACT_VERSION, current_contract_version, parse_contract_version,
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
fn contract_additive_capability_requires_minor_bump() {
    let history =
        std::fs::read_to_string("docs/cli-contract-history.toml").expect("read contract history");
    assert!(history.contains("version = \"1.0.0\""));
    assert!(history.contains("migration_note ="));
}
