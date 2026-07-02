mod common;

use std::path::Path;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};
use serde_json::{Value, json};

fn item_with_field<'a>(items: &'a Value, field: &str, value: &str) -> &'a Value {
    items
        .as_array()
        .expect("array")
        .iter()
        .find(|item| item[field] == value)
        .expect("matching item")
}

fn contains_string(items: &Value, value: &str) -> bool {
    items
        .as_array()
        .expect("array")
        .iter()
        .any(|item| item.as_str() == Some(value))
}

fn write_manifest(root: &Path, skill: &str, body: &str) {
    write_file(
        &root.join("skills").join(skill).join("loom.skill.toml"),
        body,
    );
}

#[test]
fn requirement_list_reads_nested_frontmatter_and_scalar_agent_metadata() {
    let root = TestDir::new("mcp-requirement-metadata");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\nmetadata:\n  loom:\n    requires_mcp: github\n    requires_env: FRONT_TOKEN\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/agents/codex.yaml"),
        "requires_mcp: filesystem, custom\nrequires_env: AGENT_TOKEN, SECOND_TOKEN\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "mcp",
            "requirement",
            "list",
            "--skill",
            "demo",
            "--agent",
            "codex",
        ],
    );

    assert!(
        output.status.success(),
        "requirement list should pass: {env}"
    );
    for server in ["github", "filesystem", "custom"] {
        item_with_field(&env["data"]["requirements"], "server", server);
    }
    for name in ["FRONT_TOKEN", "AGENT_TOKEN", "SECOND_TOKEN"] {
        assert_eq!(
            item_with_field(&env["data"]["env"], "name", name)["redacted"],
            json!(true)
        );
    }
}

#[test]
fn doctor_uses_table_only_mcp_requirements_without_non_mcp_plan_noise() {
    let root = TestDir::new("mcp-doctor-table-only");
    let codex_home = TestDir::new("mcp-doctor-table-only-home");
    write_skill(
        root.path(),
        "tableonly",
        "---\nname: tableonly\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root.path(),
        "tableonly",
        "[mcp.github]\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\n",
    );
    write_skill(
        root.path(),
        "toolonly",
        "---\nname: toolonly\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root.path(),
        "toolonly",
        "requires_tools = [\"missing-tool\"]\n",
    );
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "doctor", "--agent", "codex", "--skill", "tableonly"],
    );
    assert!(output.status.success(), "doctor should pass: {env}");
    assert_eq!(env["data"]["status"], json!("blocked"));
    item_with_field(&env["data"]["mcp_requirements"], "server", "github");
    assert!(contains_string(
        &env["data"]["next_actions"],
        "loom mcp plan --skill tableonly --agent codex"
    ));

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "doctor", "--agent", "codex", "--skill", "toolonly"],
    );
    assert!(output.status.success(), "doctor should pass: {env}");
    assert_eq!(env["data"]["status"], json!("blocked"));
    assert!(!contains_string(
        &env["data"]["next_actions"],
        "loom mcp plan --skill toolonly --agent codex"
    ));
}

#[test]
fn source_policy_blocks_missing_empty_and_catalog_override_sources() {
    let root = TestDir::new("mcp-source-policy");
    write_skill(
        root.path(),
        "missing",
        "---\nname: missing\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(root.path(), "missing", "requires_mcp = [\"custom\"]\n");
    write_skill(
        root.path(),
        "emptyver",
        "---\nname: emptyver\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root.path(),
        "emptyver",
        "requires_mcp = [\"custom\"]\n\n[mcp.custom]\npackage = \"npm:custom-mcp@\"\n",
    );
    write_skill(
        root.path(),
        "override",
        "---\nname: override\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root.path(),
        "override",
        "requires_mcp = [\"github\"]\n\n[mcp.github]\npackage = \"npm:some/other-mcp@1.0.0\"\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["mcp", "plan", "--skill", "missing", "--agent", "openai"],
    );
    assert!(
        output.status.success(),
        "missing-source plan should pass: {env}"
    );
    let source = item_with_field(&env["data"]["resolved_sources"], "server", "custom");
    assert_eq!(source["pinned"], json!(false));
    assert_eq!(source["approval_required"], json!("pin-mcp-source"));

    let (output, env) = run_loom(
        root.path(),
        &["mcp", "plan", "--skill", "emptyver", "--agent", "openai"],
    );
    assert!(
        output.status.success(),
        "empty-version plan should pass: {env}"
    );
    let source = item_with_field(&env["data"]["resolved_sources"], "server", "custom");
    assert_eq!(source["pinned"], json!(false));
    assert_eq!(source["version"], Value::Null);

    let (output, env) = run_loom(
        root.path(),
        &["mcp", "plan", "--skill", "override", "--agent", "openai"],
    );
    assert!(output.status.success(), "override plan should pass: {env}");
    let source = item_with_field(&env["data"]["resolved_sources"], "server", "github");
    assert_eq!(source["trust"], json!("unknown-pinned"));
    assert_eq!(source["approval_required"], json!("install-unknown-mcp"));
}

#[test]
fn plan_reports_existing_codex_config_mismatch() {
    let root = TestDir::new("mcp-config-mismatch");
    let codex_home = TestDir::new("mcp-config-mismatch-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root.path(),
        "demo",
        "requires_mcp = [\"github\"]\n\n[mcp.github]\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\nauth = \"env:GITHUB_TOKEN\"\n",
    );
    write_file(
        &codex_home.path().join("config.toml"),
        "[mcp_servers.github]\ncommand = \"other-mcp\"\nargs = [\"--stdio\"]\n",
    );
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "redacted"),
        ],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );

    assert!(output.status.success(), "mismatch plan should pass: {env}");
    let install = item_with_field(&env["data"]["actions"], "kind", "install_server");
    assert_eq!(
        install["details"]["configured"]["status"],
        json!("mismatch")
    );
    let mismatch = env["data"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["id"] == "mcp_config_mismatch")
        .expect("mismatch finding");
    assert!(contains_string(
        &mismatch["details"]["mismatches"],
        "source_package"
    ));
    assert!(contains_string(
        &mismatch["details"]["mismatches"],
        "source_version"
    ));
    assert!(contains_string(
        &mismatch["details"]["mismatches"],
        "env:GITHUB_TOKEN"
    ));
}
