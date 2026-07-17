mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};
use serde_json::{Value, json};

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

fn write_source_skill(source: &Path, name: &str, body: &str) {
    write_file(&source.join("SKILL.md"), body);
    assert!(body.contains(&format!("name: {name}")));
}

fn add_skill(root: &Path, source: &Path, name: &str) {
    let source_arg = source.to_string_lossy().to_string();
    let (output, env) = run_loom(root, &["skill", "add", &source_arg, "--name", name]);
    assert!(output.status.success(), "skill add should pass: {env}");
}

fn names(items: &Value) -> Vec<String> {
    items
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|item| item["name"].as_str().map(ToString::to_string))
        .collect()
}

fn item_named<'a>(items: &'a Value, name: &str) -> &'a Value {
    items
        .as_array()
        .expect("array")
        .iter()
        .find(|item| item["name"] == name)
        .expect("named dependency")
}

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

fn run_loom_with_home_without_codex_home(
    root: &Path,
    home: &Path,
    args: &[&str],
) -> (std::process::Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root)
        .args(args)
        .env("HOME", home)
        .env_remove("CODEX_HOME")
        .output()
        .expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

#[test]
fn skill_deps_reports_manifest_tools_env_mcp_and_network_without_secret_values() {
    let root = TestDir::new("skill-deps-manifest");
    let source = TestDir::new("skill-deps-manifest-source");
    let bin = TestDir::new("skill-deps-bin");
    let codex_home = TestDir::new("skill-deps-codex-home");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking dependencies.\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("loom.skill.toml"),
        "requires_tools = [\"fake-tool\", \"missing-tool\"]\nrequires_mcp = [\"github\"]\nrequires_env = [\"SECRET_TOKEN\"]\nnetwork = \"optional\"\n",
    );
    let fake_tool = bin.path().join("fake-tool");
    write_file(&fake_tool, "#!/usr/bin/env sh\necho fake-tool 1.2.3\n");
    make_executable(&fake_tool);
    write_file(
        &codex_home.path().join("config.toml"),
        "[mcp_servers.github]\ncommand = \"github-mcp\"\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let path = format!(
        "{}:{}",
        bin.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("PATH", &path),
            ("SECRET_TOKEN", "super-secret-value"),
            ("CODEX_HOME", &codex_home_arg),
        ],
        &["skill", "deps", "demo", "--agent", "codex"],
    );

    assert!(output.status.success(), "deps should pass: {env}");
    assert_eq!(env["cmd"], json!("skill.deps"));
    assert_eq!(env["data"]["ready"], json!(false));
    assert_eq!(
        env["data"]["dependencies"]["network"]["required"],
        json!("optional")
    );
    assert!(
        env["data"]["dependencies"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["name"] == "fake-tool" && tool["found"] == true)
    );
    assert!(
        env["data"]["dependencies"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["name"] == "missing-tool" && tool["found"] == false)
    );
    assert_eq!(
        env["data"]["dependencies"]["env"][0]["present"],
        json!(true)
    );
    assert_eq!(
        env["data"]["dependencies"]["env"][0]["redacted"],
        json!(true)
    );
    assert!(
        !serde_json::to_string(&env)
            .expect("json")
            .contains("super-secret-value")
    );
    assert_eq!(
        env["data"]["dependencies"]["mcp"][0]["configured"],
        json!(true)
    );
}

#[test]
fn skill_deps_infers_frontmatter_scripts_agent_metadata_and_unknown_mcp_agent() {
    let root = TestDir::new("skill-deps-infer");
    let source = TestDir::new("skill-deps-infer-source");
    let bin = TestDir::new("skill-deps-infer-bin");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking dependencies.\ncompatibility: Requires git, jq, Python 3.12+, and access to GitHub MCP.\nmetadata:\n  loom.requires_tools: jq\n  loom.requires_env: API_KEY\n  loom.network: optional\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("scripts/run"),
        "#!/usr/bin/env node\ngit status\ncurl https://example.com/data\n",
    );
    write_file(
        &source.path().join("agents/openai.yaml"),
        "requires_mcp:\n  - filesystem\nrequires_tools:\n  - fake-tool\n",
    );
    write_file(
        &bin.path().join("fake-tool"),
        "#!/usr/bin/env sh\necho no\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let bin_path = bin.path().to_string_lossy().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("PATH", &bin_path)],
        &["skill", "deps", "demo", "--agent", "openai"],
    );

    assert!(output.status.success(), "deps should pass: {env}");
    assert_eq!(env["data"]["ready"], json!(false));
    assert_eq!(env["data"]["status"], json!("blocked"));
    let tool_names = names(&env["data"]["dependencies"]["tools"]);
    assert!(tool_names.contains(&"git".to_string()));
    assert!(tool_names.contains(&"python".to_string()));
    assert!(tool_names.contains(&"jq".to_string()));
    assert!(tool_names.contains(&"node".to_string()));
    assert_eq!(
        item_named(&env["data"]["dependencies"]["tools"], "fake-tool")["found"],
        json!(false)
    );
    let mcp_names = names(&env["data"]["dependencies"]["mcp"]);
    assert!(mcp_names.contains(&"github".to_string()));
    assert!(mcp_names.contains(&"filesystem".to_string()));
    assert_eq!(
        env["data"]["dependencies"]["mcp"][0]["configured"],
        json!("unknown")
    );
    assert_eq!(
        env["data"]["dependencies"]["network"]["required"],
        json!("required")
    );
    assert_eq!(
        env["data"]["dependencies"]["env"][0]["redacted"],
        json!(true)
    );
}

#[test]
fn skill_deps_reports_invalid_agent_metadata_yaml() {
    let root = TestDir::new("skill-deps-invalid-agent-yaml");
    let source = TestDir::new("skill-deps-invalid-agent-yaml-source");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking invalid agent dependency metadata.\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("agents/codex.yaml"),
        "requires_mcp: [filesystem\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let (output, env) = run_loom(root.path(), &["skill", "deps", "demo", "--agent", "codex"]);

    assert!(
        output.status.success(),
        "dependency inspection should report: {env}"
    );
    assert!(
        env["data"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["id"] == "agent_metadata_yaml_invalid"),
        "invalid YAML must not be silently ignored: {env}"
    );
}

#[test]
fn unsupported_agent_mcp_status_is_not_ready_without_false_pass() {
    let root = TestDir::new("skill-deps-unknown-mcp");
    let source = TestDir::new("skill-deps-unknown-mcp-source");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking unsupported MCP agents.\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("loom.skill.toml"),
        "requires_mcp = [\"github\"]\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let (output, env) = run_loom(root.path(), &["skill", "deps", "demo", "--agent", "openai"]);

    assert!(output.status.success(), "deps should pass: {env}");
    assert_eq!(env["data"]["ready"], json!(false));
    assert_eq!(env["data"]["status"], json!("unknown"));
    assert_eq!(
        item_named(&env["data"]["dependencies"]["mcp"], "github")["configured"],
        json!("unknown")
    );
}

#[test]
fn codex_mcp_config_requires_real_server_table_from_home_config() {
    let root = TestDir::new("skill-deps-codex-mcp");
    let source = TestDir::new("skill-deps-codex-mcp-source");
    let home = TestDir::new("skill-deps-codex-home");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking MCP config parsing.\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("loom.skill.toml"),
        "requires_mcp = [\"github\"]\n",
    );
    write_file(
        &home.path().join(".codex/config.toml"),
        "# [mcp_servers.github]\nmodel = \"github\"\n[mcp_servers.filesystem]\ncommand = \"fs-mcp\"\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let (output, env) = run_loom_with_home_without_codex_home(
        root.path(),
        home.path(),
        &["skill", "deps", "demo", "--agent", "codex"],
    );

    assert!(output.status.success(), "deps should pass: {env}");
    assert_eq!(
        item_named(&env["data"]["dependencies"]["mcp"], "github")["configured"],
        json!(false)
    );
    assert_eq!(env["data"]["ready"], json!(false));
    assert!(
        env["data"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["id"] == "mcp_missing"),
        "missing MCP should be reported: {env}"
    );
}

#[test]
fn no_dependency_skill_is_ready_and_integrates_with_inspect_lint_and_diagnose() {
    let root = TestDir::new("skill-deps-none");
    let source = TestDir::new("skill-deps-none-source");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking a simple dependency-free skill.\n---\n# Demo\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let (output, env) = run_loom(root.path(), &["skill", "deps", "demo"]);
    assert!(output.status.success(), "deps should pass: {env}");
    assert_eq!(env["data"]["ready"], json!(true));
    assert_eq!(
        env["data"]["dependencies"]["tools"]
            .as_array()
            .expect("tools")
            .len(),
        0
    );

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "demo"]);
    assert!(output.status.success(), "inspect should pass: {env}");
    assert_eq!(env["data"]["dependencies"]["ready"], json!(true));

    let (output, env) = run_loom(root.path(), &["skill", "diagnose", "demo"]);
    assert!(output.status.success(), "diagnose should pass: {env}");
    assert_eq!(env["data"]["related"]["dependencies"]["ready"], json!(true));

    let (output, env) = run_loom(root.path(), &["skill", "lint", "demo", "--quality"]);
    assert!(output.status.success(), "lint should pass: {env}");
    assert!(
        env["data"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["id"] == "quality_dependencies_undeclared"),
        "lint quality should report missing declarations: {env}"
    );
}

#[test]
fn diagnose_and_lint_reflect_dependency_failures() {
    let root = TestDir::new("skill-deps-diagnose-failure");
    let source = TestDir::new("skill-deps-diagnose-failure-source");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when an agent needs to diagnose dependency readiness failures before activating a skill.\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("loom.skill.toml"),
        "requires_tools = [\"missing-tool\"]\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let (output, env) = run_loom(root.path(), &["skill", "diagnose", "demo"]);
    assert!(output.status.success(), "diagnose should pass: {env}");
    assert_eq!(env["data"]["healthy"], json!(false));
    assert_eq!(env["data"]["status"], json!("blocked"));
    assert!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|check| check["id"] == "skill_dependency:tool_missing"
                && check["severity"] == "error"),
        "diagnose checks should include dependency error: {env}"
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "demo", "--quality"]);
    assert!(output.status.success(), "lint should pass: {env}");
    assert_eq!(
        env["data"]["sections"]["quality"]["status"],
        json!("warning")
    );
    assert!(
        env["data"]["sections"]["quality"]["findings"]
            .as_array()
            .expect("quality findings")
            .iter()
            .any(|finding| finding == "quality_dependency_tool_missing"),
        "quality section should include dependency finding: {env}"
    );
}

#[test]
fn diagnose_agent_codex_passes_mcp_config_to_dependency_report() {
    let root = TestDir::new("skill-deps-diagnose-agent");
    let source = TestDir::new("skill-deps-diagnose-agent-source");
    let codex_home = TestDir::new("skill-deps-diagnose-agent-home");
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking codex MCP readiness.\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("loom.skill.toml"),
        "requires_mcp = [\"github\"]\n",
    );
    write_file(
        &codex_home.path().join("config.toml"),
        "[mcp_servers.github]\ncommand = \"github-mcp\"\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let codex_home_arg = codex_home.path().to_string_lossy().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["skill", "diagnose", "demo", "--agent", "codex"],
    );

    assert!(output.status.success(), "diagnose should pass: {env}");
    assert_eq!(
        item_named(
            &env["data"]["related"]["dependencies"]["dependencies"]["mcp"],
            "github"
        )["configured"],
        json!(true)
    );
    assert_eq!(env["data"]["related"]["dependencies"]["ready"], json!(true));
}

#[test]
fn missing_skill_returns_typed_error() {
    let root = TestDir::new("skill-deps-missing");

    let (output, env) = run_loom(root.path(), &["skill", "deps", "missing"]);

    assert!(!output.status.success(), "deps should fail: {env}");
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));
}

#[test]
fn mcp_requirement_list_merges_sources_and_redacts_secret_values() {
    let root = TestDir::new("mcp-requirements");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when planning MCP requirements.\ncompatibility: Needs GitHub MCP for issue triage.\nmetadata:\n  loom.requires_mcp: filesystem\n  loom.requires_env: FRONT_TOKEN\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_mcp = [\"github\"]\nrequires_env = [\"ROOT_TOKEN\"]\n\n[mcp.github]\nrequired = true\ntransport = \"stdio\"\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\nauth = \"env:GITHUB_TOKEN\"\npermissions = [\"repo:read\"]\n",
    );
    write_file(
        &root.path().join("skills/demo/agents/codex.yaml"),
        "requires_mcp:\n  - local-index\nrequires_env:\n  - AGENT_TOKEN\n",
    );

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("GITHUB_TOKEN", "super-secret-value")],
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
    assert_eq!(env["cmd"], json!("mcp.requirement.list"));
    for server in ["github", "filesystem", "local-index"] {
        item_with_field(&env["data"]["requirements"], "server", server);
    }
    let github = item_with_field(&env["data"]["requirements"], "server", "github");
    assert_eq!(github["auth_env"], json!("GITHUB_TOKEN"));
    assert!(contains_string(&github["permissions"], "repo:read"));
    assert_eq!(
        item_with_field(&env["data"]["env"], "name", "GITHUB_TOKEN")["redacted"],
        json!(true)
    );
    assert!(
        !serde_json::to_string(&env)
            .expect("json")
            .contains("super-secret-value")
    );
    assert!(
        env["data"]["findings"]
            .as_array()
            .expect("findings")
            .iter()
            .any(|finding| finding["id"] == "mcp_requirement_suggestion")
    );
}

#[test]
fn mcp_plan_is_read_only_and_records_policy_env_tool_and_config_actions() {
    let root = TestDir::new("mcp-plan");
    let workspace = TestDir::new("mcp-plan-workspace");
    let codex_home = TestDir::new("mcp-plan-codex-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when planning MCP config.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_mcp = [\"github\"]\n\n[mcp.github]\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\nauth = \"env:GITHUB_TOKEN\"\n",
    );

    let codex_home_arg = codex_home.path().to_string_lossy().to_string();
    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "plan",
            "--skill",
            "demo",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(output.status.success(), "mcp plan should pass: {env}");
    assert_eq!(env["cmd"], json!("mcp.plan"));
    assert_eq!(env["data"]["writes_performed"], json!(false));
    assert!(!codex_home.path().join("config.toml").exists());
    let source = item_with_field(&env["data"]["resolved_sources"], "server", "github");
    assert_eq!(
        source["package"],
        json!("@modelcontextprotocol/server-github")
    );
    assert_eq!(source["version"], json!("0.6.2"));
    assert_eq!(source["policy"], json!("approval_required"));
    assert!(contains_string(
        &env["data"]["approvals_required"],
        "install-third-party-mcp"
    ));
    assert!(contains_string(
        &env["data"]["approvals_required"],
        "write-agent-mcp-config"
    ));
    let install = item_with_field(&env["data"]["actions"], "kind", "install_server");
    assert_eq!(install["details"]["configured"]["status"], json!("missing"));
    assert_eq!(install["details"]["configured"]["present"], json!(false));
    let config = item_with_field(&env["data"]["actions"], "kind", "write_agent_config");
    assert_eq!(config["diff_redacted"], json!(true));
    assert!(contains_string(
        &config["depends_on"],
        "install_server:github"
    ));
    assert!(contains_string(
        &config["depends_on"],
        "require_env:GITHUB_TOKEN"
    ));
    assert!(contains_string(&config["depends_on"], "tool:npx"));
    assert_eq!(
        item_with_field(&env["data"]["actions"], "kind", "require_env")["present"],
        json!(true)
    );
    assert!(
        !serde_json::to_string(&env)
            .expect("json")
            .contains("super-secret-value")
    );
}

#[test]
fn mcp_catalog_source_policy_and_manual_agent_mode_are_explicit() {
    let root = TestDir::new("mcp-catalog-policy");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when planning unknown MCP sources.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_mcp = [\"custom\"]\n\n[mcp.custom]\npackage = \"npm:@scope/custom-mcp\"\n",
    );

    let (output, env) = run_loom(root.path(), &["mcp", "catalog", "show", "github"]);
    assert!(output.status.success(), "catalog show should pass: {env}");
    assert_eq!(env["data"]["entry"]["required_tool"], json!("npx"));
    assert_eq!(env["data"]["entry"]["transport"], json!("stdio"));

    let (output, env) = run_loom(
        root.path(),
        &["mcp", "plan", "--skill", "demo", "--agent", "openai"],
    );
    assert!(output.status.success(), "manual plan should pass: {env}");
    let source = item_with_field(&env["data"]["resolved_sources"], "server", "custom");
    assert_eq!(source["policy"], json!("blocked_unpinned"));
    assert_eq!(source["approval_required"], json!("pin-mcp-source"));
    let manual = item_with_field(
        &env["data"]["actions"],
        "kind",
        "manual_configuration_required",
    );
    assert_eq!(manual["server"], json!("custom"));
    assert!(manual["path"].is_null());

    let (output, env) = run_loom(root.path(), &["mcp", "doctor", "--agent", "openai"]);
    assert!(output.status.success(), "manual doctor should pass: {env}");
    assert_eq!(
        env["data"]["status"],
        json!("manual_configuration_required")
    );
    assert_eq!(
        env["data"]["next_actions"][0],
        json!("run loom mcp doctor --skill <skill> --agent <agent>")
    );
}

#[test]
fn mcp_doctor_and_skill_diagnose_point_to_plan_when_readiness_fails() {
    let root = TestDir::new("mcp-doctor-diagnose");
    let codex_home = TestDir::new("mcp-doctor-codex-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking MCP doctor next actions.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_mcp = [\"github\"]\n",
    );
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "doctor", "--agent", "codex", "--skill", "demo"],
    );
    assert!(output.status.success(), "mcp doctor should pass: {env}");
    assert_eq!(env["data"]["status"], json!("blocked"));
    assert!(contains_string(
        &env["data"]["next_actions"],
        "loom mcp plan --skill demo --agent codex"
    ));

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["skill", "diagnose", "demo", "--agent", "codex"],
    );
    assert!(output.status.success(), "skill diagnose should pass: {env}");
    let readiness = item_with_field(&env["data"]["checks"], "id", "dependency_readiness");
    assert!(contains_string(
        &readiness["details"]["next_actions"],
        "loom mcp plan --skill demo --agent codex"
    ));
}
