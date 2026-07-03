mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{TestDir, run_loom_with_env, write_file, write_skill};
use serde_json::{Value, json};

#[path = "../src/sha256.rs"]
mod sha256;

fn write_manifest(root: &Path, skill: &str, body: &str) {
    write_file(
        &root.join("skills").join(skill).join("loom.skill.toml"),
        body,
    );
}

fn write_github_skill(root: &Path, skill: &str) {
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

fn digest_suffix(value: &str) -> String {
    digest_bytes(value.as_bytes())
        .strip_prefix("sha256:")
        .expect("sha prefix")
        .to_string()
}

fn plan_id(env: &Value) -> String {
    env["data"]["plan_id"]
        .as_str()
        .expect("plan id")
        .to_string()
}

fn apply_github(root: &TestDir, codex_home: &TestDir, plan_id: &str, key: &str) -> Value {
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();
    let (output, env) = run_loom_with_env(
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
            key,
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );
    assert!(output.status.success(), "mcp apply should pass: {env}");
    env
}

fn apply_record_path(root: &TestDir, key: &str) -> PathBuf {
    root.path()
        .join("state/mcp/applies")
        .join(format!("{}.json", digest_suffix(key)))
}

fn config_lock_path(root: &TestDir, config_path: &Path) -> PathBuf {
    root.path().join("state/mcp/locks").join(format!(
        "config_{}.lock",
        digest_suffix(&config_path.display().to_string())
    ))
}

#[test]
fn mcp_apply_preserves_unknown_server_settings_and_forwards_env_vars() {
    let root = TestDir::new("mcp-apply-preserve-settings");
    let codex_home = TestDir::new("mcp-apply-preserve-settings-home");
    write_github_skill(root.path(), "demo");
    write_file(
        &codex_home.path().join("config.toml"),
        "[mcp_servers.github]\ncommand = \"old\"\ntimeout_ms = 5000\ndisabled = true\n\n[mcp_servers.github.env]\nSTATIC_VALUE = \"kept\"\n",
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
    apply_github(&root, &codex_home, &plan_id(&plan_env), "preserve-settings");

    let config = fs::read_to_string(codex_home.path().join("config.toml")).expect("config");
    assert!(config.contains("command = \"npx\""));
    assert!(config.contains("env_vars = [\"GITHUB_TOKEN\"]"));
    assert!(config.contains("timeout_ms = 5000"));
    assert!(config.contains("disabled = true"));
    assert!(config.contains("STATIC_VALUE = \"kept\""));
    assert!(!config.contains("env:GITHUB_TOKEN"));
    assert!(!config.contains("super-secret-value"));
}

#[test]
fn mcp_apply_recovers_missing_record_when_config_already_matches() {
    let root = TestDir::new("mcp-apply-record-recovery");
    let codex_home = TestDir::new("mcp-apply-record-recovery-home");
    write_github_skill(root.path(), "demo");
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
    let plan_id = plan_id(&plan_env);
    apply_github(&root, &codex_home, &plan_id, "recover-record");
    fs::remove_file(apply_record_path(&root, "recover-record")).expect("remove record");

    let (recover_output, recover_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            &plan_id,
            "--idempotency-key",
            "recover-record",
        ],
    );

    assert!(
        recover_output.status.success(),
        "mcp apply should recover missing record without volatile gates: {recover_env}"
    );
    assert_eq!(recover_env["data"]["idempotent_replay"], json!(true));
    assert_eq!(recover_env["data"]["target_writes_performed"], json!(false));
    assert!(apply_record_path(&root, "recover-record").is_file());
}

#[test]
fn mcp_apply_rejects_artifact_source_tampering_and_mutable_npm_specs() {
    let root = TestDir::new("mcp-apply-source-tamper");
    let codex_home = TestDir::new("mcp-apply-source-tamper-home");
    write_github_skill(root.path(), "demo");
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
    let reviewed_plan_id = plan_id(&plan_env);
    let durable_path = root
        .path()
        .join(format!("state/mcp/plans/{reviewed_plan_id}.json"));
    let mut artifact: Value =
        serde_json::from_str(&fs::read_to_string(&durable_path).expect("plan")).expect("json");
    artifact["resolved_sources"][0]["locator"] = json!("npm:other-mcp@1.0.0");
    artifact["resolved_sources"][0]["package"] = json!("other-mcp");
    artifact["resolved_sources"][0]["version"] = json!("1.0.0");
    fs::write(
        &durable_path,
        serde_json::to_string_pretty(&artifact).expect("json"),
    )
    .expect("write tampered plan");

    let (tamper_output, tamper_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "apply",
            &reviewed_plan_id,
            "--idempotency-key",
            "tampered-source",
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );
    assert!(
        !tamper_output.status.success(),
        "tampered source should be blocked: {tamper_env}"
    );
    assert_eq!(tamper_env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(!codex_home.path().join("config.toml").exists());

    write_skill(
        root.path(),
        "mutable",
        "---\nname: mutable\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(
        root.path(),
        "mutable",
        "requires_mcp = [\"custom\"]\n\n[mcp.custom]\npackage = \"npm:custom-mcp@latest\"\n",
    );
    let (mutable_plan_output, mutable_plan_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &["mcp", "plan", "--skill", "mutable", "--agent", "codex"],
    );
    assert!(
        mutable_plan_output.status.success(),
        "mutable plan should report blocked source: {mutable_plan_env}"
    );
    assert_eq!(
        mutable_plan_env["data"]["resolved_sources"][0]["pinned"],
        json!(false)
    );
    let mutable_id = plan_id(&mutable_plan_env);
    let (mutable_apply_output, mutable_apply_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            &mutable_id,
            "--idempotency-key",
            "mutable-source",
            "--approve",
            "pin-mcp-source",
            "--approve",
            "write-agent-mcp-config",
        ],
    );
    assert!(
        !mutable_apply_output.status.success(),
        "mutable npm source should be blocked: {mutable_apply_env}"
    );
    assert_eq!(mutable_apply_env["error"]["code"], json!("POLICY_BLOCKED"));
}

#[test]
fn mcp_apply_skips_optional_mcp_and_renders_absolute_local_commands() {
    let root = TestDir::new("mcp-apply-optional-local");
    let codex_home = TestDir::new("mcp-apply-optional-local-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    let server = root.path().join("skills/demo/bin/local-mcp");
    let server_body = "#!/bin/sh\necho local\n";
    write_file(&server, server_body);
    let locator = format!(
        "local:bin/local-mcp@{}",
        digest_bytes(server_body.as_bytes())
    );
    write_manifest(
        root.path(),
        "demo",
        &format!(
            "requires_mcp = [\"local\"]\n\n[mcp.local]\npackage = \"{locator}\"\n\n[mcp.github]\nrequired = false\npackage = \"npm:@modelcontextprotocol/server-github@0.6.2\"\nauth = \"env:GITHUB_TOKEN\"\n"
        ),
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
    assert_eq!(
        plan_env["data"]["requirements"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        plan_env["data"]["requirements"][0]["server"],
        json!("local")
    );
    assert!(
        plan_env["data"]["resolved_sources"][0]["locator"]
            .as_str()
            .expect("locator")
            .starts_with("local:/"),
        "relative local locator should be reviewed as absolute: {plan_env}"
    );

    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            plan_env["data"]["plan_id"].as_str().expect("plan id"),
            "--idempotency-key",
            "optional-local",
            "--approve",
            "install-unknown-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );

    assert!(
        apply_output.status.success(),
        "mcp apply should pass: {apply_env}"
    );
    let config = fs::read_to_string(codex_home.path().join("config.toml")).expect("config");
    let reviewed_server = fs::canonicalize(&server).expect("canonical server");
    assert!(config.contains(&format!("command = \"{}\"", reviewed_server.display())));
    assert!(!config.contains("[mcp_servers.github]"));
    assert!(!config.contains("GITHUB_TOKEN"));
}

#[test]
fn mcp_apply_scopes_filesystem_server_to_reviewed_workspace() {
    let root = TestDir::new("mcp-apply-filesystem-scope");
    let workspace = TestDir::new("mcp-apply-filesystem-workspace");
    let codex_home = TestDir::new("mcp-apply-filesystem-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_manifest(root.path(), "demo", "requires_mcp = [\"filesystem\"]\n");
    let codex_home_arg = codex_home.path().to_string_lossy().to_string();
    let workspace_arg = workspace.path().to_string_lossy().to_string();

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
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
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    assert_eq!(
        plan_env["data"]["workspace"],
        json!(workspace.path().display().to_string())
    );
    let (apply_output, apply_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", &codex_home_arg)],
        &[
            "mcp",
            "apply",
            plan_env["data"]["plan_id"].as_str().expect("plan id"),
            "--idempotency-key",
            "filesystem-scope",
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
    let config = fs::read_to_string(codex_home.path().join("config.toml")).expect("config");
    assert!(config.contains("@modelcontextprotocol/server-filesystem@0.6.2"));
    assert!(config.contains(&workspace.path().display().to_string()));
}

#[test]
fn mcp_plan_rejects_output_inside_skill_source_and_records_absolute_config_path() {
    let root = TestDir::new("mcp-plan-output-boundary");
    write_github_skill(root.path(), "demo");
    let output_plan = root.path().join("skills/demo/plan.json");
    let output_plan_arg = output_plan.to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", "relative-codex-home")],
        &[
            "mcp",
            "plan",
            "--skill",
            "demo",
            "--agent",
            "codex",
            "--output-plan",
            &output_plan_arg,
        ],
    );
    assert!(
        !output.status.success(),
        "plan output inside skill source should fail: {env}"
    );
    assert_eq!(env["error"]["code"], json!("ARG_INVALID"));
    assert!(!output_plan.exists());

    let (plan_output, plan_env) = run_loom_with_env(
        root.path(),
        &[("CODEX_HOME", "relative-codex-home")],
        &["mcp", "plan", "--skill", "demo", "--agent", "codex"],
    );
    assert!(
        plan_output.status.success(),
        "mcp plan should pass: {plan_env}"
    );
    let write_action = plan_env["data"]["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .find(|action| action["kind"] == "write_agent_config")
        .expect("write action");
    let planned_path = write_action["path"].as_str().expect("planned path");
    assert!(Path::new(planned_path).is_absolute(), "{planned_path}");
    assert!(
        planned_path.ends_with("relative-codex-home/config.toml"),
        "{planned_path}"
    );
}

#[test]
fn mcp_apply_uses_config_scoped_lock_and_reaps_stale_key_locks() {
    let root = TestDir::new("mcp-apply-locks");
    let codex_home = TestDir::new("mcp-apply-locks-home");
    write_github_skill(root.path(), "demo");
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
    let plan_id = plan_id(&plan_env);
    let config_path = codex_home.path().join("config.toml");
    write_file(
        &config_lock_path(&root, &config_path),
        &format!(r#"{{"pid":{}}}"#, std::process::id()),
    );

    let (busy_output, busy_env) = run_loom_with_env(
        root.path(),
        &[
            ("CODEX_HOME", &codex_home_arg),
            ("GITHUB_TOKEN", "super-secret-value"),
        ],
        &[
            "mcp",
            "apply",
            &plan_id,
            "--idempotency-key",
            "busy-config",
            "--approve",
            "install-third-party-mcp",
            "--approve",
            "write-agent-mcp-config",
        ],
    );
    assert!(
        !busy_output.status.success(),
        "active config lock should block apply: {busy_env}"
    );
    assert_eq!(busy_env["error"]["code"], json!("POLICY_BLOCKED"));
    fs::remove_file(config_lock_path(&root, &config_path)).expect("remove config lock");

    write_file(
        &root
            .path()
            .join("state/mcp/applies")
            .join(format!("{}.lock", digest_suffix("stale-key"))),
        r#"{"pid":999999}"#,
    );
    let apply_env = apply_github(&root, &codex_home, &plan_id, "stale-key");
    assert_eq!(apply_env["data"]["target_writes_performed"], json!(true));
}
