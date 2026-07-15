use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

mod common;

use common::{TestDir, run_loom_with_env, write_file, write_skill};

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
    let home = home.to_string_lossy().to_string();
    run_loom_with_env(root, &[("HOME", &home)], args)
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
    let target_dir = home.join(".claude/skills");
    fs::create_dir_all(&target_dir).expect("create claude target");
    let projection = target_dir.join(skill);
    symlink_skill(&root.join("skills").join(skill), &projection);

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
                "target_id": "target_claude_user",
                "agent": "claude",
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
                "binding_id": "bind_claude_user",
                "agent": "claude",
                "profile_id": "default",
                "workspace_matcher": {"kind": "name", "value": "default"},
                "default_target_id": "target_claude_user",
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
                "binding_id": "bind_claude_user",
                "skill_id": skill,
                "target_id": "target_claude_user",
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
                "instance_id": format!("inst_{skill}_claude_user"),
                "skill_id": skill,
                "binding_id": "bind_claude_user",
                "target_id": "target_claude_user",
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
