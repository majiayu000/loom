use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod common;

use common::{TestDir, write_file, write_skill};

fn write_good_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when testing Codex visibility.\n---\n# {skill}\n"
        ),
    );
}

fn run_with_home(root: &Path, home: &Path, args: &[&str]) -> (std::process::Output, Value) {
    run_with_home_and_env(root, home, &[], args)
}

fn run_with_home_and_env(
    root: &Path,
    home: &Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> (std::process::Output, Value) {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_loom"));
    cmd.arg("--json").arg("--root").arg(root).args(args);
    for key in [
        "GEMINI_CLI_HOME",
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        "GEMINI_CLI_TRUSTED_FOLDERS_PATH",
        "GEMINI_CLI_TRUST_WORKSPACE",
    ] {
        cmd.env_remove(key);
    }
    cmd.env("HOME", home);
    cmd.env(
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        home.join("missing-system-defaults.json"),
    );
    cmd.env(
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        home.join("missing-system-settings.json"),
    );
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

fn activate(root: &Path, home: &Path, skill: &str) {
    let (output, env) = run_with_home(
        root,
        home,
        &["skill", "activate", skill, "--agent", "codex"],
    );
    assert!(
        output.status.success(),
        "activate {skill} failed: stdout={} stderr={} env={env}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn codex_config_path(home: &Path) -> std::path::PathBuf {
    home.join(".codex/config.toml")
}

fn write_json(path: &Path, value: Value) {
    let mut body = serde_json::to_string_pretty(&value).expect("serialize json");
    body.push('\n');
    write_file(path, &body);
}

fn symlink_skill(source: &Path, projection: &Path) {
    if let Some(parent) = projection.parent() {
        fs::create_dir_all(parent).expect("create projection parent");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, projection).expect("symlink skill projection");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(source, projection).expect("symlink skill projection");
}

fn write_claude_visibility_state(root: &Path, home: &Path, skill: &str) -> PathBuf {
    write_agent_visibility_state(
        root,
        &home.join(".claude/skills"),
        skill,
        "claude",
        "user",
        json!({"kind": "name", "value": "default"}),
    )
}

fn write_agent_visibility_state(
    root: &Path,
    target_dir: &Path,
    skill: &str,
    agent: &str,
    suffix: &str,
    workspace_matcher: Value,
) -> PathBuf {
    fs::create_dir_all(target_dir).expect("create agent target");
    let projection = target_dir.join(skill);
    symlink_skill(&root.join("skills").join(skill), &projection);
    let target_id = format!("target_{agent}_{suffix}");
    let binding_id = format!("bind_{agent}_{suffix}");

    let registry = root.join("state/registry");
    write_json(
        &registry.join("schema.json"),
        json!({
            "schema_version": 1,
            "created_at": "2026-07-06T00:00:00Z",
            "writer": "loom/test"
        }),
    );
    write_json(
        &registry.join("targets.json"),
        json!({
            "schema_version": 1,
            "targets": [{
                "target_id": target_id,
                "agent": agent,
                "path": target_dir,
                "ownership": "managed",
                "capabilities": {"symlink": true, "copy": true, "watch": true},
                "created_at": "2026-07-06T00:00:00Z"
            }]
        }),
    );
    write_json(
        &registry.join("bindings.json"),
        json!({
            "schema_version": 1,
            "bindings": [{
                "binding_id": binding_id,
                "agent": agent,
                "profile_id": "default",
                "workspace_matcher": workspace_matcher,
                "default_target_id": target_id,
                "policy_profile": "safe-capture",
                "active": true,
                "created_at": "2026-07-06T00:00:00Z"
            }]
        }),
    );
    write_json(
        &registry.join("rules.json"),
        json!({
            "schema_version": 1,
            "rules": [{
                "binding_id": binding_id,
                "skill_id": skill,
                "target_id": target_id,
                "method": "symlink",
                "watch_policy": "observe_only",
                "created_at": "2026-07-06T00:00:00Z"
            }]
        }),
    );
    write_json(
        &registry.join("projections.json"),
        json!({
            "schema_version": 1,
            "projections": [{
                "instance_id": format!("inst_{skill}_{agent}_{suffix}"),
                "skill_id": skill,
                "binding_id": binding_id,
                "target_id": target_id,
                "materialized_path": projection,
                "method": "symlink",
                "last_applied_rev": "abc123",
                "health": "healthy",
                "observed_drift": false,
                "updated_at": "2026-07-06T00:00:00Z"
            }]
        }),
    );
    write_json(
        &registry.join("ops/checkpoint.json"),
        json!({
            "schema_version": 1,
            "last_scanned_op_id": null,
            "last_acked_op_id": null,
            "updated_at": "2026-07-06T00:00:00Z"
        }),
    );
    write_file(&registry.join("ops/operations.jsonl"), "");
    projection
}

fn append_agent_visibility_state(
    root: &Path,
    target_dir: &Path,
    skill: &str,
    agent: &str,
    suffix: &str,
    workspace_matcher: Value,
) -> PathBuf {
    fs::create_dir_all(target_dir).expect("create agent target");
    let projection = target_dir.join(skill);
    symlink_skill(&root.join("skills").join(skill), &projection);
    let target_id = format!("target_{agent}_{suffix}");
    let binding_id = format!("bind_{agent}_{suffix}");
    let registry = root.join("state/registry");
    for (file, key, row) in [
        (
            "targets.json",
            "targets",
            json!({
                "target_id": target_id,
                "agent": agent,
                "path": target_dir,
                "ownership": "managed",
                "capabilities": {"symlink": true, "copy": true, "watch": true},
                "created_at": "2026-07-06T00:00:00Z"
            }),
        ),
        (
            "bindings.json",
            "bindings",
            json!({
                "binding_id": binding_id,
                "agent": agent,
                "profile_id": "default",
                "workspace_matcher": workspace_matcher,
                "default_target_id": target_id,
                "policy_profile": "safe-capture",
                "active": true,
                "created_at": "2026-07-06T00:00:00Z"
            }),
        ),
        (
            "rules.json",
            "rules",
            json!({
                "binding_id": binding_id,
                "skill_id": skill,
                "target_id": target_id,
                "method": "symlink",
                "watch_policy": "observe_only",
                "created_at": "2026-07-06T00:00:00Z"
            }),
        ),
        (
            "projections.json",
            "projections",
            json!({
                "instance_id": format!("inst_{skill}_{agent}_{suffix}"),
                "skill_id": skill,
                "binding_id": binding_id,
                "target_id": target_id,
                "materialized_path": projection,
                "method": "symlink",
                "last_applied_rev": "abc123",
                "health": "healthy",
                "observed_drift": false,
                "updated_at": "2026-07-06T00:00:00Z"
            }),
        ),
    ] {
        let path = registry.join(file);
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read state"))
                .expect("parse state");
        value[key].as_array_mut().expect("state rows").push(row);
        write_json(&path, value);
    }
    projection
}

fn rewrite_registry_agent(root: &Path, agent: &str) {
    for (file, array_key) in [("targets.json", "targets"), ("bindings.json", "bindings")] {
        let path = root.join("state/registry").join(file);
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read registry json"))
                .expect("parse registry json");
        value[array_key][0]["agent"] = json!(agent);
        write_json(&path, value);
    }
}

fn action_categories(env: &Value) -> Vec<String> {
    env["data"]["plans"][0]["actions"]
        .as_array()
        .expect("actions array")
        .iter()
        .filter_map(|action| action["category"].as_str().map(str::to_string))
        .collect()
}

fn check_ids(env: &Value) -> Vec<String> {
    env["data"]["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter_map(|check| check["id"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn skill_visibility_reports_path_and_name_config_disables() {
    let root = TestDir::new("codex-visibility-disable");
    let home = TestDir::new("codex-visibility-disable-home");
    write_good_skill(root.path(), "demo");
    activate(root.path(), home.path(), "demo");

    let skill_file = root.path().join("skills/demo/SKILL.md");
    write_file(
        &codex_config_path(home.path()),
        &format!(
            r#"[[skills.config]]
path = "{}"
enabled = false

[[skills.config]]
name = "demo"
enabled = false
"#,
            skill_file.display()
        ),
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "codex"],
    );

    assert!(output.status.success(), "visibility should succeed: {env}");
    assert_eq!(env["data"]["visible"], Value::Bool(false));
    assert_eq!(
        env["data"]["convergence"]["projections"]["state"],
        json!("converged")
    );
    assert_eq!(
        env["data"]["convergence"]["visibility"]["state"],
        json!("restart_required")
    );
    let checks = env["data"]["checks"].as_array().expect("checks");
    for id in [
        "codex_config_not_disabled_by_path",
        "codex_config_not_disabled_by_name",
    ] {
        let check = checks
            .iter()
            .find(|check| check["id"] == Value::String(id.to_string()))
            .unwrap_or_else(|| panic!("missing check {id}: {checks:?}"));
        assert_eq!(check["ok"], Value::Bool(false), "{id} should fail");
        assert_eq!(check["severity"], Value::String("error".to_string()));
    }
}

#[test]
fn skill_visibility_claude_uses_adapter_metadata() {
    let root = TestDir::new("claude-visibility");
    let home = TestDir::new("claude-visibility-home");
    write_good_skill(root.path(), "demo");
    write_claude_visibility_state(root.path(), home.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "claude"],
    );

    assert!(
        output.status.success(),
        "claude visibility should pass: {env}"
    );
    assert_eq!(env["data"]["agent"], json!("claude"));
    assert_eq!(env["data"]["visible"], Value::Bool(true));
    let ids = check_ids(&env);
    for id in [
        "claude_active_rule_exists",
        "claude_projection_points_to_source:target_claude_user",
        "claude_config_metadata_available",
        "claude_disable_rules_adapter_defined",
    ] {
        assert!(ids.contains(&id.to_string()), "missing {id}: {env}");
    }
}

#[test]
fn skill_diagnose_claude_attaches_visibility_report() {
    let root = TestDir::new("claude-diagnose");
    let home = TestDir::new("claude-diagnose-home");
    write_good_skill(root.path(), "demo");
    write_claude_visibility_state(root.path(), home.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "diagnose", "demo", "--agent", "claude"],
    );

    assert!(
        output.status.success(),
        "claude diagnose should pass: {env}"
    );
    assert_eq!(
        env["data"]["related"]["agent_visibility"]["agent"],
        json!("claude")
    );
    assert_eq!(
        env["data"]["related"]["claude_visibility"]["visible"],
        Value::Bool(true)
    );
    let has_claude_section = env["data"]["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .any(|check| check["section"] == json!("claude"));
    assert!(
        has_claude_section,
        "diagnose should include claude checks: {env}"
    );
}

#[test]
fn agent_reconcile_claude_dry_run_reports_missing_projection_without_mutation() {
    let root = TestDir::new("claude-reconcile-dry-run");
    let home = TestDir::new("claude-reconcile-dry-run-home");
    write_good_skill(root.path(), "demo");
    let projected = write_claude_visibility_state(root.path(), home.path(), "demo");
    fs::remove_file(&projected).expect("remove projection");
    let projections_before =
        fs::read_to_string(root.path().join("state/registry/projections.json"))
            .expect("read projections before");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["agent", "reconcile", "--agent", "claude", "--dry-run"],
    );

    assert!(
        output.status.success(),
        "claude reconcile should pass: {env}"
    );
    assert_eq!(env["data"]["plans"][0]["agent"], json!("claude"));
    assert!(
        action_categories(&env).contains(&"create_projection".to_string()),
        "dry-run should report create_projection: {env}"
    );
    assert!(
        !projected.exists(),
        "dry-run must not recreate missing projection"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/projections.json"))
            .expect("read projections after"),
        projections_before,
        "dry-run must not mutate registry projections"
    );
}

#[test]
fn skill_visibility_returns_structured_unsupported_for_adapter_without_metadata() {
    let root = TestDir::new("visibility-unsupported");
    let home = TestDir::new("visibility-unsupported-home");
    write_good_skill(root.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "cursor"],
    );

    assert!(
        output.status.success(),
        "unsupported visibility should be structured success: {env}"
    );
    assert_eq!(env["data"]["agent"], json!("cursor"));
    assert_eq!(env["data"]["visible"], Value::Bool(false));
    let checks = env["data"]["checks"].as_array().expect("checks");
    assert!(
        checks
            .iter()
            .any(|check| check["id"] == json!("visibility_unsupported")),
        "missing visibility_unsupported check: {env}"
    );
    let unsupported = checks
        .iter()
        .find(|check| check["id"] == json!("visibility_unsupported"))
        .expect("unsupported check");
    assert!(
        unsupported["message"]
            .as_str()
            .is_some_and(|message| message.contains("generic fidelity")),
        "generic adapters must not be presented as verified: {unsupported}"
    );
    assert_eq!(unsupported["details"]["fidelity"], "generic");
}

#[test]
fn skill_diagnose_reports_generic_adapter_fidelity() {
    let root = TestDir::new("diagnose-generic-fidelity");
    let home = TestDir::new("diagnose-generic-fidelity-home");
    write_good_skill(root.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "diagnose", "demo", "--agent", "cursor"],
    );

    assert!(output.status.success(), "cursor diagnose failed: {env}");
    let checks = env["data"]["checks"].as_array().expect("checks");
    let unsupported = checks
        .iter()
        .find(|check| check["id"] == json!("visibility_unsupported"))
        .expect("generic diagnose visibility check");
    assert!(
        unsupported["message"]
            .as_str()
            .is_some_and(|message| message.contains("generic fidelity")),
        "diagnose must explain the generic adapter limitation: {unsupported}"
    );
    assert_eq!(unsupported["details"]["fidelity"], "generic");
    assert_eq!(
        env["data"]["related"]["agent_visibility"]["fidelity"],
        "generic"
    );
}

#[test]
fn skill_visibility_gemini_cli_uses_verified_adapter_metadata() {
    let root = TestDir::new("visibility-gemini-cli-verified");
    let home = TestDir::new("visibility-gemini-cli-verified-home");
    write_good_skill(root.path(), "demo");
    write_agent_visibility_state(
        root.path(),
        &home.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "user",
        json!({"kind": "name", "value": "default"}),
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );

    assert!(output.status.success(), "Gemini visibility failed: {env}");
    assert_eq!(env["data"]["agent"], json!("gemini-cli"));
    assert_eq!(env["data"]["visible"], Value::Bool(false));
    let checks = env["data"]["checks"].as_array().expect("checks");
    assert!(
        checks
            .iter()
            .all(|check| check["id"] != json!("visibility_unsupported")),
        "verified Gemini metadata must drive visibility checks: {env}"
    );
    for id in [
        "gemini-cli_config_valid",
        "gemini-cli_skills_enabled",
        "gemini-cli_skill_not_disabled",
        "gemini-cli_frontmatter_valid:target_gemini-cli_user",
    ] {
        assert!(
            checks
                .iter()
                .any(|check| check["id"] == id && check["ok"] == true),
            "missing passing Gemini check {id}: {env}"
        );
    }
    let admin = checks
        .iter()
        .find(|check| check["id"] == "gemini-cli_admin_policy_observable")
        .expect("admin policy check");
    assert_eq!(admin["ok"], false);
    assert!(
        admin["message"]
            .as_str()
            .is_some_and(|message| message.contains("admin policy"))
    );
    assert!(
        admin["next_action"]
            .as_str()
            .is_some_and(|action| action.contains("/skills list"))
    );
    assert!(
        env["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action
                .as_str()
                .is_some_and(|action| action.contains("/skills list")))
    );
    let reload = checks
        .iter()
        .find(|check| check["id"] == "gemini-cli_reload_required")
        .expect("Gemini reload check");
    assert!(
        reload["message"]
            .as_str()
            .is_some_and(|message| message.contains("/skills reload"))
    );
    assert_eq!(reload["details"]["hot_reload"], true);
    assert_eq!(reload["details"]["strategy"], "in-session-command");
}

#[test]
fn skill_visibility_gemini_cli_unions_case_insensitive_disables() {
    let root = TestDir::new("visibility-gemini-cli-disabled");
    let home = TestDir::new("visibility-gemini-cli-disabled-home");
    write_good_skill(root.path(), "demo");
    write_agent_visibility_state(
        root.path(),
        &home.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "user",
        json!({"kind": "name", "value": "default"}),
    );
    let system_defaults = root.path().join("gemini-system-defaults.json");
    write_json(&system_defaults, json!({"skills": {"disabled": ["Demo"]}}));
    write_json(
        &home.path().join(".gemini/settings.json"),
        json!({"skills": {"disabled": []}}),
    );
    let system_defaults_str = system_defaults.display().to_string();

    let (output, env) = run_with_home_and_env(
        root.path(),
        home.path(),
        &[("GEMINI_CLI_SYSTEM_DEFAULTS_PATH", &system_defaults_str)],
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(output.status.success(), "Gemini visibility failed: {env}");
    assert_eq!(env["data"]["visible"], false);
    assert_eq!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["id"] == "gemini-cli_skill_not_disabled")
            .expect("disabled check")["ok"],
        false
    );
    let next_actions = env["data"]["next_actions"]
        .as_array()
        .expect("next actions");
    assert!(
        next_actions.iter().all(|action| {
            action.as_str().is_none_or(|action| {
                !action.contains("codex reconcile") && !action.contains("restart Codex")
            })
        }),
        "Gemini diagnostics must not suggest Codex actions: {env}"
    );

    write_json(
        &home.path().join(".gemini/settings.json"),
        json!({"admin": {"skills": {"enabled": false}}}),
    );
    let system_settings = root.path().join("gemini-system-settings.json");
    write_json(
        &system_settings,
        json!({"admin": {"skills": {"enabled": false}}}),
    );
    let system_settings_str = system_settings.display().to_string();
    let (output, env) = run_with_home_and_env(
        root.path(),
        home.path(),
        &[("GEMINI_CLI_SYSTEM_SETTINGS_PATH", &system_settings_str)],
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        output.status.success(),
        "Gemini local-admin report failed: {env}"
    );
    assert_eq!(env["data"]["visible"], false);
    assert_eq!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["id"] == "gemini-cli_admin_policy_observable")
            .expect("admin check")["ok"],
        false
    );

    write_file(&home.path().join(".gemini/settings.json"), "{not-json\n");
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        output.status.success(),
        "malformed config report failed: {env}"
    );
    assert_eq!(env["data"]["visible"], false);
    assert_eq!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["id"] == "gemini-cli_config_valid")
            .expect("config validity check")["ok"],
        false
    );
    assert!(
        env["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action
                .as_str()
                .is_some_and(|action| action.contains("repair Gemini CLI settings")))
    );
}

#[test]
fn skill_visibility_gemini_cli_accepts_comments_in_official_json_files() {
    let root = TestDir::new("visibility-gemini-cli-json-comments");
    let home = TestDir::new("visibility-gemini-cli-json-comments-home");
    write_good_skill(root.path(), "demo");
    write_agent_visibility_state(
        root.path(),
        &home.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "user",
        json!({"kind": "name", "value": "default"}),
    );
    write_file(
        &home.path().join(".gemini/settings.json"),
        "{\n  // Gemini CLI settings support comments.\n  \"skills\": {\"enabled\": true, \"disabled\": []},\n  /* preserve comment-like text inside strings */\n  \"note\": \"https://example.test/*literal*/\"\n}\n",
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(output.status.success(), "commented settings failed: {env}");
    assert_eq!(env["data"]["visible"], false);
    assert_eq!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["id"] == "gemini-cli_config_valid")
            .expect("config validity check")["ok"],
        true
    );
}

#[test]
fn skill_visibility_gemini_cli_rejects_invalid_projected_frontmatter() {
    let root = TestDir::new("visibility-gemini-cli-frontmatter");
    let home = TestDir::new("visibility-gemini-cli-frontmatter-home");
    write_good_skill(root.path(), "demo");
    write_agent_visibility_state(
        root.path(),
        &home.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "user",
        json!({"kind": "name", "value": "default"}),
    );
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "# missing frontmatter\n",
    );
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "visibility", "demo", "--agent", "gemini-cli"],
    );
    assert!(output.status.success(), "frontmatter report failed: {env}");
    assert_eq!(env["data"]["visible"], false);
    assert!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|check| {
                check["id"] == "gemini-cli_frontmatter_valid:target_gemini-cli_user"
                    && check["ok"] == false
            })
    );
    assert!(
        env["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action
                .as_str()
                .is_some_and(|action| action.contains("frontmatter")))
    );
}

#[test]
fn skill_visibility_gemini_cli_requires_trusted_project_workspace() {
    let root = TestDir::new("visibility-gemini-cli-project-trust");
    let home = TestDir::new("visibility-gemini-cli-project-trust-home");
    let workspace = TestDir::new("visibility-gemini-cli-project-workspace");
    write_good_skill(root.path(), "demo");
    let workspace_str = workspace.path().display().to_string();
    write_agent_visibility_state(
        root.path(),
        &workspace.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "project",
        json!({"kind": "path_prefix", "value": workspace_str}),
    );
    write_json(
        &home.path().join(".gemini/settings.json"),
        json!({"security": {"folderTrust": {"enabled": true}}}),
    );
    write_json(
        &home.path().join(".gemini/trustedFolders.json"),
        json!({workspace.path().display().to_string(): "DO_NOT_TRUST"}),
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "visibility",
            "demo",
            "--agent",
            "gemini-cli",
            "--workspace",
            &workspace.path().display().to_string(),
        ],
    );
    assert!(
        output.status.success(),
        "untrusted visibility failed: {env}"
    );
    assert_eq!(env["data"]["visible"], false);
    let trust_check = env["data"]["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["id"] == "gemini-cli_workspace_trusted")
        .expect("workspace trust check");
    assert_eq!(trust_check["ok"], false);
    assert_eq!(trust_check["details"]["trusted"], false);
    assert!(
        trust_check["next_action"]
            .as_str()
            .is_some_and(|action| action.contains("/permissions trust"))
    );

    write_file(
        &home.path().join(".gemini/trustedFolders.json"),
        "{not-json\n",
    );
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "visibility",
            "demo",
            "--agent",
            "gemini-cli",
            "--workspace",
            &workspace.path().display().to_string(),
        ],
    );
    assert!(
        output.status.success(),
        "malformed trust report failed: {env}"
    );
    assert_eq!(env["data"]["visible"], false);
    assert_eq!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["id"] == "gemini-cli_config_valid")
            .expect("config validity check")["ok"],
        false
    );

    write_file(
        &home.path().join(".gemini/trustedFolders.json"),
        &format!(
            "{{\n  // Gemini CLI trust files support comments.\n  {:?}: \"TRUST_FOLDER\"\n}}\n",
            workspace.path().display().to_string()
        ),
    );
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "visibility",
            "demo",
            "--agent",
            "gemini-cli",
            "--workspace",
            &workspace.path().display().to_string(),
        ],
    );
    assert!(output.status.success(), "trusted visibility failed: {env}");
    let checks = env["data"]["checks"].as_array().expect("checks");
    assert!(
        checks
            .iter()
            .any(|check| { check["id"] == "gemini-cli_workspace_trusted" && check["ok"] == true })
    );
    assert_eq!(env["data"]["visible"], false);
}

#[test]
fn skill_visibility_gemini_cli_keeps_valid_user_projection_independent_of_project_trust() {
    let root = TestDir::new("visibility-gemini-cli-user-project");
    let home = TestDir::new("visibility-gemini-cli-user-project-home");
    let workspace = TestDir::new("visibility-gemini-cli-user-project-workspace");
    write_good_skill(root.path(), "demo");
    write_agent_visibility_state(
        root.path(),
        &workspace.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "project",
        json!({"kind": "path_prefix", "value": workspace.path()}),
    );
    append_agent_visibility_state(
        root.path(),
        &home.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "user",
        json!({"kind": "name", "value": "default"}),
    );
    write_json(
        &home.path().join(".gemini/trustedFolders.json"),
        json!({workspace.path().display().to_string(): "DO_NOT_TRUST"}),
    );
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "visibility",
            "demo",
            "--agent",
            "gemini-cli",
            "--workspace",
            workspace.path().to_str().expect("workspace"),
        ],
    );
    assert!(output.status.success(), "mixed-scope report failed: {env}");
    assert!(
        env["data"]["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .all(|check| check["id"] != "gemini-cli_workspace_trusted")
    );
}

#[test]
fn agent_reconcile_gemini_cli_reports_in_session_reload() {
    let root = TestDir::new("reconcile-gemini-reload");
    let home = TestDir::new("reconcile-gemini-reload-home");
    write_good_skill(root.path(), "demo");
    let projection = write_agent_visibility_state(
        root.path(),
        &home.path().join(".agents/skills"),
        "demo",
        "gemini-cli",
        "user",
        json!({"kind": "name", "value": "default"}),
    );
    fs::remove_file(projection).expect("remove projection");
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["agent", "reconcile", "--agent", "gemini-cli", "--dry-run"],
    );
    assert!(output.status.success(), "Gemini reconcile failed: {env}");
    assert_eq!(env["data"]["plans"][0]["restart_required"], false);
    assert!(
        env["data"]["plans"][0]["warnings"]
            .as_array()
            .expect("warnings")
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("/skills reload")))
    );
}

#[test]
fn agent_reconcile_returns_structured_unsupported_without_visibility_metadata() {
    let root = TestDir::new("reconcile-unsupported");
    let home = TestDir::new("reconcile-unsupported-home");
    write_good_skill(root.path(), "demo");
    write_claude_visibility_state(root.path(), home.path(), "demo");
    rewrite_registry_agent(root.path(), "cursor");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["agent", "reconcile", "--agent", "cursor", "--dry-run"],
    );

    assert!(
        output.status.success(),
        "unsupported reconcile should be structured success: {env}"
    );
    assert_eq!(env["data"]["plans"].as_array().map(Vec::len), Some(0));
    assert_eq!(env["data"]["unsupported"], Value::Bool(true));
    assert_eq!(
        env["data"]["checks"][0]["id"],
        json!("visibility_unsupported")
    );
}

#[test]
fn codex_reconcile_dry_run_reports_missing_projection_without_mutation() {
    let root = TestDir::new("codex-reconcile-dry-run");
    let home = TestDir::new("codex-reconcile-dry-run-home");
    write_good_skill(root.path(), "demo");
    activate(root.path(), home.path(), "demo");
    let projected = home.path().join(".agents/skills/demo");
    fs::remove_file(&projected).expect("remove projection");
    let projections_before =
        fs::read_to_string(root.path().join("state/registry/projections.json"))
            .expect("read projections before");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["codex", "reconcile", "--dry-run"],
    );

    assert!(output.status.success(), "dry-run should pass: {env}");
    assert!(
        action_categories(&env).contains(&"create_projection".to_string()),
        "dry-run should report create_projection: {env}"
    );
    assert!(
        !projected.exists(),
        "dry-run must not recreate missing projection"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("state/registry/projections.json"))
            .expect("read projections after"),
        projections_before,
        "dry-run must not mutate registry projections"
    );
}

#[test]
fn codex_reconcile_apply_repairs_projection_without_editing_config() {
    let root = TestDir::new("codex-reconcile-apply");
    let home = TestDir::new("codex-reconcile-apply-home");
    write_good_skill(root.path(), "demo");
    activate(root.path(), home.path(), "demo");
    let projected = home.path().join(".agents/skills/demo");
    fs::remove_file(&projected).expect("remove projection");
    write_file(
        &codex_config_path(home.path()),
        "[[skills.config]]\nname = \"demo\"\nenabled = false\n",
    );

    let (output, env) = run_with_home(root.path(), home.path(), &["codex", "reconcile", "--apply"]);

    assert!(output.status.success(), "apply should pass: {env}");
    assert!(
        fs::symlink_metadata(&projected)
            .expect("projection metadata")
            .file_type()
            .is_symlink(),
        "apply should recreate symlink projection"
    );
    let config = fs::read_to_string(codex_config_path(home.path())).expect("read config");
    assert!(
        config.contains("enabled = false"),
        "--apply without --fix-config must not edit Codex config: {config}"
    );
}

#[test]
fn codex_reconcile_apply_fix_config_patches_disabled_entry() {
    let root = TestDir::new("codex-reconcile-fix-config");
    let home = TestDir::new("codex-reconcile-fix-config-home");
    write_good_skill(root.path(), "demo");
    activate(root.path(), home.path(), "demo");
    write_file(
        &codex_config_path(home.path()),
        "title = \"keep me\"\n\n[[skills.config]]\nname = \"demo\"\nenabled = false\n",
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["codex", "reconcile", "--apply", "--fix-config"],
    );

    assert!(output.status.success(), "fix-config should pass: {env}");
    assert_eq!(env["data"]["restart_required"], Value::Bool(true));
    let config = fs::read_to_string(codex_config_path(home.path())).expect("read config");
    assert!(config.contains("title = \"keep me\""));
    assert!(
        config.contains("enabled = true"),
        "fix-config should flip only the disabled entry: {config}"
    );
}

#[test]
fn codex_reconcile_preserves_runtime_external_and_shared_target_union() {
    let root = TestDir::new("codex-reconcile-union");
    let home = TestDir::new("codex-reconcile-union-home");
    for skill in ["alpha", "beta", "old"] {
        write_good_skill(root.path(), skill);
        activate(root.path(), home.path(), skill);
    }
    let target_dir = home.path().join(".agents/skills");
    fs::create_dir_all(target_dir.join(".system")).expect("runtime dir");
    fs::create_dir_all(target_dir.join("external-tool")).expect("external dir");

    let bindings_path = root.path().join("state/registry/bindings.json");
    let mut bindings: Value =
        serde_json::from_str(&fs::read_to_string(&bindings_path).expect("read bindings"))
            .expect("parse bindings");
    let mut extra_binding = bindings["bindings"][0].clone();
    extra_binding["binding_id"] = Value::String("bind_codex_extra_user".to_string());
    bindings["bindings"]
        .as_array_mut()
        .unwrap()
        .push(extra_binding);
    fs::write(
        &bindings_path,
        serde_json::to_string_pretty(&bindings).expect("write bindings json"),
    )
    .expect("write bindings");

    let rules_path = root.path().join("state/registry/rules.json");
    let mut rules: Value =
        serde_json::from_str(&fs::read_to_string(&rules_path).expect("read rules"))
            .expect("parse rules");
    rules["rules"]
        .as_array_mut()
        .unwrap()
        .retain(|rule| rule["skill_id"] != Value::String("old".to_string()));
    for rule in rules["rules"].as_array_mut().unwrap() {
        if rule["skill_id"] == Value::String("beta".to_string()) {
            rule["binding_id"] = Value::String("bind_codex_extra_user".to_string());
        }
    }
    fs::write(
        &rules_path,
        serde_json::to_string_pretty(&rules).expect("write rules json"),
    )
    .expect("write rules");

    let projections_path = root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_str(&fs::read_to_string(&projections_path).expect("read projections"))
            .expect("parse projections");
    for projection in projections["projections"].as_array_mut().unwrap() {
        if projection["skill_id"] == Value::String("beta".to_string()) {
            projection["binding_id"] = Value::String("bind_codex_extra_user".to_string());
        }
    }
    fs::write(
        &projections_path,
        serde_json::to_string_pretty(&projections).expect("write projections json"),
    )
    .expect("write projections");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "codex",
            "reconcile",
            "--apply",
            "--binding",
            "bind_codex_default_user",
        ],
    );

    assert!(output.status.success(), "union apply should pass: {env}");
    assert!(
        target_dir.join("beta").exists(),
        "shared target union must preserve beta"
    );
    assert!(
        !target_dir.join("old").exists(),
        "stale Loom-owned symlink should be removed"
    );
    assert!(
        target_dir.join(".system").exists(),
        "runtime entry must be preserved"
    );
    assert!(
        target_dir.join("external-tool").exists(),
        "external entry must be preserved"
    );
    let categories = action_categories(&env);
    assert!(categories.contains(&"remove_stale_projection".to_string()));
    assert!(categories.contains(&"preserve_runtime_entry".to_string()));
    assert!(categories.contains(&"preserve_external_entry".to_string()));
}

#[test]
fn codex_reconcile_fix_config_malformed_toml_is_typed_error() {
    let root = TestDir::new("codex-reconcile-malformed-config");
    let home = TestDir::new("codex-reconcile-malformed-config-home");
    write_good_skill(root.path(), "demo");
    activate(root.path(), home.path(), "demo");
    write_file(
        &codex_config_path(home.path()),
        "[[skills.config]]\nname =\n",
    );

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["codex", "reconcile", "--apply", "--fix-config"],
    );

    assert!(!output.status.success(), "malformed config should fail");
    assert_eq!(env["error"]["code"], json!("SCHEMA_MISMATCH"));
}
