mod common;

use std::{fs, path::Path};

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};
use serde_json::{Value, json};

#[path = "../src/sha256.rs"]
mod sha256;

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

fn write_github_mcp_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root,
        skill,
        "requires_mcp = [\"github\"]\n\n[mcp.github]\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\nauth = \"env:GITHUB_TOKEN\"\n",
    );
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = sha256::Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", sha256::to_hex(&hasher.finalize()))
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

#[test]
fn mcp_apply_writes_codex_config_and_replays_idempotently() {
    let root = TestDir::new("mcp-apply-root");
    let codex_home = TestDir::new("mcp-apply-codex-home");
    write_github_mcp_skill(root.path(), "demo");

    let codex_home_arg = codex_home.path().to_string_lossy().to_string();
    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    assert_eq!(plan_env["data"]["durable_plan_written"], json!(true));
    let plan_id = plan_env["data"]["plan_id"].as_str().expect("plan id");

    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "mcp-key-1",
            "--approve",
            "install-third-party-mcp,write-agent-mcp-config",
        ],
    );

    assert!(
        apply_output.status.success(),
        "mcp apply should pass: {apply_env}"
    );
    assert_eq!(apply_env["cmd"], json!("mcp.apply"));
    assert_eq!(apply_env["data"]["target_writes_performed"], json!(true));
    assert_eq!(apply_env["data"]["idempotent_replay"], json!(false));
    assert_eq!(apply_env["data"]["secret_values_written"], json!(false));
    assert!(
        apply_env["data"]["idempotency_key_digest"]
            .as_str()
            .expect("digest")
            .starts_with("sha256:")
    );
    assert!(
        !serde_json::to_string(&apply_env)
            .expect("json")
            .contains("mcp-key-1")
    );

    let config = fs::read_to_string(codex_home.path().join("config.toml")).expect("config");
    assert!(config.contains("[mcp_servers.github]"));
    assert!(config.contains("command = \"npx\""));
    assert!(config.contains("@modelcontextprotocol/server-github@0.6.2"));
    assert!(config.contains("env_vars = [\"GITHUB_TOKEN\"]"));
    assert!(!config.contains("env:GITHUB_TOKEN"));
    assert!(!config.contains("super-secret-value"));

    let (replay_output, replay_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "apply", plan_id, "--idempotency-key", "mcp-key-1"],
    );

    assert!(
        replay_output.status.success(),
        "mcp apply replay should pass: {replay_env}"
    );
    assert_eq!(replay_env["data"]["idempotent_replay"], json!(true));
    assert_eq!(replay_env["data"]["target_writes_performed"], json!(false));
}

#[test]
fn mcp_apply_requires_approvals_and_env_without_writes() {
    let root = TestDir::new("mcp-apply-guards");
    let codex_home = TestDir::new("mcp-apply-guards-home");
    write_github_mcp_skill(root.path(), "demo");
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    let plan_id = plan_env["data"]["plan_id"].as_str().expect("plan id");

    let (approval_output, approval_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "missing-approval",
        ],
    );
    assert!(
        !approval_output.status.success(),
        "mcp apply should require approvals: {approval_env}"
    );
    assert_eq!(approval_env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        approval_env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert!(!codex_home.path().join("config.toml").exists());

    let (env_output, env_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "missing-env",
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );
    assert!(
        !env_output.status.success(),
        "mcp apply should require env: {env_env}"
    );
    assert_eq!(env_env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(contains_string(
        &env_env["error"]["details"]["missing_env"],
        "GITHUB_TOKEN"
    ));
    assert!(!codex_home.path().join("config.toml").exists());
}

#[test]
fn mcp_apply_rejects_stale_config_preimage() {
    let root = TestDir::new("mcp-apply-stale");
    let codex_home = TestDir::new("mcp-apply-stale-home");
    write_github_mcp_skill(root.path(), "demo");
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    let plan_id = plan_env["data"]["plan_id"].as_str().expect("plan id");
    write_file(&codex_home.path().join("config.toml"), "# user change\n");

    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "stale-config",
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );

    assert!(
        !apply_output.status.success(),
        "mcp apply should reject stale config: {apply_env}"
    );
    assert_eq!(apply_env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        fs::read_to_string(codex_home.path().join("config.toml")).expect("config"),
        "# user change\n"
    );
}

#[test]
fn mcp_apply_rejects_changed_local_source_digest() {
    let root = TestDir::new("mcp-apply-local-digest");
    let codex_home = TestDir::new("mcp-apply-local-digest-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    let server = root.path().join("bin/local-mcp");
    let original = b"#!/bin/sh\necho original\n";
    write_file(&server, std::str::from_utf8(original).expect("utf8"));
    let locator = format!("local:{}@{}", server.display(), digest_bytes(original));
    write_manifest(
        root.path(),
        "demo",
        &format!("[mcp.local]\npackage = \"{locator}\"\n"),
    );
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    let plan_id = plan_env["data"]["plan_id"].as_str().expect("plan id");
    write_file(&server, "#!/bin/sh\necho changed\n");

    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "changed-local-source",
            "--approve",
            "install-unknown-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );

    assert!(
        !apply_output.status.success(),
        "mcp apply should reject changed local source: {apply_env}"
    );
    assert_eq!(apply_env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        apply_env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert!(!codex_home.path().join("config.toml").exists());
}

#[test]
fn mcp_apply_overwrites_fuzzy_compatible_command() {
    let root = TestDir::new("mcp-apply-fuzzy-compatible");
    let codex_home = TestDir::new("mcp-apply-fuzzy-compatible-home");
    write_github_mcp_skill(root.path(), "demo");
    write_file(
        &codex_home.path().join("config.toml"),
        "[mcp_servers.github]\ncommand = \"evil\"\nargs = [\"@modelcontextprotocol/server-github@0.6.2\"]\n\n[mcp_servers.github.env]\nGITHUB_TOKEN = \"env:GITHUB_TOKEN\"\n",
    );
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    let install = item_with_field(&plan_env["data"]["actions"], "kind", "install_server");
    assert_eq!(
        install["details"]["configured"]["status"],
        json!("compatible")
    );
    let plan_id = plan_env["data"]["plan_id"].as_str().expect("plan id");

    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "fuzzy-compatible-command",
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );

    assert!(
        apply_output.status.success(),
        "mcp apply should pass: {apply_env}"
    );
    assert_eq!(apply_env["data"]["target_writes_performed"], json!(true));
    let config = fs::read_to_string(codex_home.path().join("config.toml")).expect("config");
    assert!(config.contains("command = \"npx\""));
    assert!(!config.contains("command = \"evil\""));
    assert!(config.contains("env_vars = [\"GITHUB_TOKEN\"]"));
    assert!(!config.contains("env:GITHUB_TOKEN"));
}

#[cfg(unix)]
#[test]
fn mcp_apply_rejects_stale_symlinked_skill_source() {
    let root = TestDir::new("mcp-apply-symlink-digest");
    let codex_home = TestDir::new("mcp-apply-symlink-digest-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    let manifest_target = root.path().join("linked-manifest.toml");
    write_file(
        &manifest_target,
        "requires_mcp = [\"github\"]\n\n[mcp.github]\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\nauth = \"env:GITHUB_TOKEN\"\n",
    );
    std::os::unix::fs::symlink(
        &manifest_target,
        root.path().join("skills/demo/loom.skill.toml"),
    )
    .expect("symlink manifest");
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    let plan_id = plan_env["data"]["plan_id"].as_str().expect("plan id");
    write_file(
        &manifest_target,
        "requires_mcp = [\"github\"]\n\n[mcp.github]\npackage = \"npm:@modelcontextprotocol/server-github@0.6.3\"\nauth = \"env:GITHUB_TOKEN\"\n",
    );

    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "apply",
            plan_id,
            "--idempotency-key",
            "stale-symlink-source",
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );

    assert!(
        !apply_output.status.success(),
        "mcp apply should reject stale symlinked source: {apply_env}"
    );
    assert_eq!(apply_env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        apply_env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert!(!codex_home.path().join("config.toml").exists());
}
