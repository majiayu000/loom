mod common;

use std::fs;
use std::path::Path;

use common::{TestDir, run_loom, run_loom_with_env, write_file};
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
    write_source_skill(
        source.path(),
        "demo",
        "---\nname: demo\ndescription: Use when checking dependencies.\ncompatibility: Requires Python and access to GitHub MCP.\nmetadata:\n  loom.requires_tools: jq\n  loom.requires_env: API_KEY\n  loom.network: optional\n---\n# Demo\n",
    );
    write_file(
        &source.path().join("scripts/run"),
        "#!/usr/bin/env node\ncurl https://example.com/data\n",
    );
    write_file(
        &source.path().join("agents/openai.yaml"),
        "requires_mcp: filesystem\n",
    );
    add_skill(root.path(), source.path(), "demo");

    let (output, env) = run_loom(root.path(), &["skill", "deps", "demo", "--agent", "openai"]);

    assert!(output.status.success(), "deps should pass: {env}");
    let tool_names = names(&env["data"]["dependencies"]["tools"]);
    assert!(tool_names.contains(&"python".to_string()));
    assert!(tool_names.contains(&"jq".to_string()));
    assert!(tool_names.contains(&"node".to_string()));
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
fn missing_skill_returns_typed_error() {
    let root = TestDir::new("skill-deps-missing");

    let (output, env) = run_loom(root.path(), &["skill", "deps", "missing"]);

    assert!(!output.status.success(), "deps should fail: {env}");
    assert_eq!(env["error"]["code"], json!("SKILL_NOT_FOUND"));
}
