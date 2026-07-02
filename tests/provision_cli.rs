mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::{TestDir, run_loom, run_loom_in_cwd, write_file, write_skill};
use serde_json::{Value, json};

fn write_codex_registry_state(root: &TestDir, workspace: &TestDir) {
    write_codex_registry_state_for_workspace(root, workspace.path());
}

fn write_codex_registry_state_for_workspace(root: &TestDir, workspace: &Path) {
    let registry = root.path().join("state/registry");
    write_file(
        &registry.join("schema.json"),
        r#"{"schema_version":1,"created_at":"2026-04-09T10:00:00Z","writer":"test"}
"#,
    );
    write_file(
        &registry.join("targets.json"),
        r#"{"schema_version":1,"targets":[{"target_id":"target_codex_project","agent":"codex","path":"/tmp/codex-project/.agents/skills","ownership":"managed","capabilities":{"symlink":true,"copy":true,"watch":true},"created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &registry.join("bindings.json"),
        &format!(
            r#"{{"schema_version":1,"bindings":[{{"binding_id":"bind_codex_project","agent":"codex","profile_id":"default","workspace_matcher":{{"kind":"path_prefix","value":"{}"}},"default_target_id":"target_codex_project","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"}}]}}
"#,
            workspace.display()
        ),
    );
    write_file(
        &registry.join("rules.json"),
        r#"{"schema_version":1,"rules":[{"binding_id":"bind_codex_project","skill_id":"demo","target_id":"target_codex_project","method":"copy","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &registry.join("projections.json"),
        r#"{"schema_version":1,"projections":[{"instance_id":"inst_demo_codex_project","skill_id":"demo","binding_id":"bind_codex_project","target_id":"target_codex_project","materialized_path":"/tmp/codex-project/.agents/skills/demo","method":"copy","last_applied_rev":"abc123","health":"healthy","observed_drift":false,"updated_at":"2026-04-09T10:05:00Z"}]}
"#,
    );
    write_file(
        &registry.join("ops/checkpoint.json"),
        r#"{"schema_version":1,"last_scanned_op_id":null,"last_acked_op_id":null,"updated_at":"2026-04-09T10:07:00Z"}
"#,
    );
    write_file(&registry.join("ops/operations.jsonl"), "");
}

fn commit_all(root: &TestDir) {
    let add = Command::new("git")
        .current_dir(root.path())
        .args(["add", "."])
        .output()
        .expect("git add");
    assert!(add.status.success(), "git add failed");
    let commit = Command::new("git")
        .current_dir(root.path())
        .args([
            "-c",
            "user.name=Loom Test",
            "-c",
            "user.email=loom-test@example.com",
            "commit",
            "-m",
            "seed registry",
        ])
        .output()
        .expect("git commit");
    assert!(
        commit.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
}

fn file_plan<'a>(files: &'a Value, path: &str) -> &'a Value {
    files
        .as_array()
        .expect("files array")
        .iter()
        .find(|file| file["path"] == path)
        .expect("matching file plan")
}

#[test]
fn provision_plan_devcontainer_is_read_only_and_uses_codex_project_path() {
    let root = TestDir::new("provision-plan-root");
    let workspace = TestDir::new("provision-plan-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_env = [\"DEMO_TOKEN\"]\n",
    );
    write_codex_registry_state(&root, &workspace);
    Command::new("git")
        .arg("init")
        .arg(root.path())
        .output()
        .expect("git init");
    commit_all(&root);
    Command::new("git")
        .current_dir(root.path())
        .args([
            "remote",
            "add",
            "origin",
            "git+https://token@example.com/org/loom-registry.git",
        ])
        .output()
        .expect("git remote add");

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--agent",
            "codex",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(output.status.success(), "provision plan failed: {env}");
    assert_eq!(env["cmd"], json!("provision.plan"));
    assert_eq!(env["data"]["target_writes_performed"], json!(false));
    assert!(!workspace.path().join(".devcontainer").exists());

    let plan = &env["data"]["plan"];
    assert_eq!(plan["target_kind"], json!("devcontainer"));
    assert_eq!(
        plan["registry_clone_url"],
        json!("https://example.com/org/loom-registry.git")
    );
    assert_eq!(
        plan["registry_source_display"],
        json!("https://example.com/org/loom-registry.git")
    );
    assert_eq!(
        plan["active_views"][0]["path"],
        json!(format!(
            "/workspaces/{}/.agents/skills",
            workspace.path().file_name().unwrap().to_string_lossy()
        ))
    );
    assert_eq!(plan["active_views"][0]["skills"], json!(["demo"]));
    assert_eq!(plan["secrets_required"][0]["name"], json!("DEMO_TOKEN"));
    assert_eq!(plan["secrets_required"][0]["redacted"], json!(true));
    assert!(
        plan["guards"]["active_view_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );

    let setup = file_plan(&plan["files_to_write"], ".devcontainer/loom-setup.sh");
    assert!(
        setup["preview"]
            .as_str()
            .unwrap()
            .contains("set -euo pipefail")
    );
    assert!(setup["preview"].as_str().unwrap().contains("git clone"));
    assert!(
        setup["preview"]
            .as_str()
            .unwrap()
            .contains("git -C \"$LOOM_REGISTRY_DIR\" checkout --detach")
    );
    assert!(setup["preview"].as_str().unwrap().contains("exit 1"));
    assert!(!setup["preview"].as_str().unwrap().contains("|| echo"));
    assert!(!setup["preview"].as_str().unwrap().contains("token@"));
}

#[test]
fn provision_plan_resolves_relative_workspace_from_caller_cwd() {
    let root = TestDir::new("provision-relative-root");
    let parent = TestDir::new("provision-relative-parent");
    let workspace = parent.path().join("app");
    fs::create_dir_all(&workspace).expect("create workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state_for_workspace(&root, &workspace);

    let (output, env) = run_loom_in_cwd(
        root.path(),
        &workspace,
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            ".",
        ],
    );

    assert!(output.status.success(), "relative workspace failed: {env}");
    assert_eq!(
        env["data"]["plan"]["workspace"],
        json!(fs::canonicalize(&workspace).unwrap().display().to_string())
    );
    assert_eq!(
        env["data"]["plan"]["active_views"][0]["skills"],
        json!(["demo"])
    );
}

#[test]
fn provision_plan_path_prefix_matches_path_components() {
    let root = TestDir::new("provision-prefix-root");
    let parent = TestDir::new("provision-prefix-parent");
    let workspace = parent.path().join("app");
    let sibling = parent.path().join("app2");
    fs::create_dir_all(&workspace).expect("create workspace");
    fs::create_dir_all(&sibling).expect("create sibling");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state_for_workspace(&root, &workspace);

    let sibling_arg = sibling.to_string_lossy().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            &sibling_arg,
        ],
    );

    assert!(output.status.success(), "sibling workspace failed: {env}");
    assert_eq!(env["data"]["plan"]["active_views"][0]["skills"], json!([]));
    assert!(
        env["data"]["plan"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["id"] == "active_binding_missing")
    );
}

#[test]
fn provision_plan_sanitizes_clone_urls_and_treats_local_remotes_as_local_only() {
    let root = TestDir::new("provision-url-root");
    let workspace = TestDir::new("provision-url-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state(&root, &workspace);
    Command::new("git")
        .arg("init")
        .arg(root.path())
        .output()
        .expect("git init");
    Command::new("git")
        .current_dir(root.path())
        .args([
            "remote",
            "add",
            "origin",
            "https://example.com/org/repo@v2.git?token=secret",
        ])
        .output()
        .expect("git remote add");

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(output.status.success(), "url provision plan failed: {env}");
    let plan = &env["data"]["plan"];
    assert_eq!(
        plan["registry_clone_url"],
        json!("https://example.com/org/repo@v2.git")
    );
    assert_eq!(
        plan["registry_source_display"],
        json!("https://example.com/org/repo@v2.git")
    );
    assert!(
        plan["secrets_required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|secret| secret["name"] == "GIT_CREDENTIALS")
    );
    let serialized_plan = serde_json::to_string(plan).unwrap();
    assert!(!serialized_plan.contains("token=secret"));
    assert!(!serialized_plan.contains("?token"));

    let local_root = TestDir::new("provision-local-remote-root");
    let local_workspace = TestDir::new("provision-local-remote-workspace");
    let local_remote = TestDir::new("provision-local-remote-origin");
    write_skill(
        local_root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state(&local_root, &local_workspace);
    Command::new("git")
        .arg("init")
        .arg(local_root.path())
        .output()
        .expect("git init local root");
    Command::new("git")
        .current_dir(local_root.path())
        .args([
            "remote",
            "add",
            "origin",
            &local_remote.path().display().to_string(),
        ])
        .output()
        .expect("git remote add local");

    let local_workspace_arg = local_workspace.path().to_string_lossy().to_string();
    let (local_output, local_env) = run_loom(
        local_root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            &local_workspace_arg,
        ],
    );

    assert!(
        local_output.status.success(),
        "local remote provision plan failed: {local_env}"
    );
    let local_plan = &local_env["data"]["plan"];
    assert!(local_plan["registry_clone_url"].is_null());
    assert!(
        local_plan["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["id"] == "registry_remote_local_only")
    );
    let setup = file_plan(&local_plan["files_to_write"], ".devcontainer/loom-setup.sh");
    assert!(!setup["preview"].as_str().unwrap().contains("git clone /"));
}

#[test]
fn provision_plan_uses_external_adapter_project_root_and_rejects_unsafe_agents() {
    let root = TestDir::new("provision-adapter-root");
    let workspace = TestDir::new("provision-adapter-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("adapters/fixture-v2.json"),
        r#"{
  "adapter_api": "2",
  "id": "fixture-v2",
  "supported_scopes": ["project"],
  "projection_methods": ["copy"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {
    "automatic_discovery": true,
    "explicit_invocation": true,
    "reload_required": false
  },
  "discovery_roots": [
    {
      "scope": "project",
      "path": "<workspace>/.fixture-v2/skills",
      "role": "project-cross-client",
      "scan_eligible": false
    }
  ],
  "visibility": {
    "follows_symlink_dirs": true,
    "identity_by_projection_method": {
      "copy": "runtime-skill-md-path"
    }
  },
  "reload": {
    "strategy": "new-session-recommended",
    "hot_reload": false
  }
}
"#,
    );
    let registry = root.path().join("state/registry");
    write_file(
        &registry.join("schema.json"),
        r#"{"schema_version":1,"created_at":"2026-04-09T10:00:00Z","writer":"test"}
"#,
    );
    write_file(
        &registry.join("targets.json"),
        r#"{"schema_version":1,"targets":[{"target_id":"target_fixture_project","agent":"fixture-v2","path":"/tmp/fixture-v2/skills","ownership":"managed","capabilities":{"symlink":true,"copy":true,"watch":true},"created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &registry.join("bindings.json"),
        &format!(
            r#"{{"schema_version":1,"bindings":[{{"binding_id":"bind_fixture_project","agent":"fixture-v2","profile_id":"default","workspace_matcher":{{"kind":"path_prefix","value":"{}"}},"default_target_id":"target_fixture_project","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"}}]}}
"#,
            workspace.path().display()
        ),
    );
    write_file(
        &registry.join("rules.json"),
        r#"{"schema_version":1,"rules":[{"binding_id":"bind_fixture_project","skill_id":"demo","target_id":"target_fixture_project","method":"copy","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &registry.join("projections.json"),
        r#"{"schema_version":1,"projections":[]}"#,
    );
    write_file(
        &registry.join("ops/checkpoint.json"),
        r#"{"schema_version":1,"last_scanned_op_id":null,"last_acked_op_id":null,"updated_at":"2026-04-09T10:07:00Z"}
"#,
    );
    write_file(&registry.join("ops/operations.jsonl"), "");

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--agent",
            "fixture-v2",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(
        output.status.success(),
        "adapter provision plan failed: {env}"
    );
    assert_eq!(
        env["data"]["plan"]["active_views"][0]["path"],
        json!(format!(
            "/workspaces/{}/.fixture-v2/skills",
            workspace.path().file_name().unwrap().to_string_lossy()
        ))
    );

    let (bad_output, bad_env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--agent",
            "bad;agent",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(!bad_output.status.success(), "unsafe agent should fail");
    assert_eq!(bad_env["error"]["code"], json!("ARG_INVALID"));
}

#[test]
fn provision_plan_splits_active_views_by_rule_target() {
    let root = TestDir::new("provision-rule-target-root");
    let workspace = TestDir::new("provision-rule-target-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_skill(
        root.path(),
        "alt",
        "---\nname: alt\ndescription: Alt.\n---\n# Alt\n",
    );
    let registry = root.path().join("state/registry");
    write_file(
        &registry.join("schema.json"),
        r#"{"schema_version":1,"created_at":"2026-04-09T10:00:00Z","writer":"test"}
"#,
    );
    write_file(
        &registry.join("targets.json"),
        &format!(
            r#"{{"schema_version":1,"targets":[{{"target_id":"target_codex_project","agent":"codex","path":"/tmp/codex-project/.agents/skills","ownership":"managed","capabilities":{{"symlink":true,"copy":true,"watch":true}},"created_at":"2026-04-09T10:00:00Z"}},{{"target_id":"target_codex_alt","agent":"codex","path":"{}","ownership":"managed","capabilities":{{"symlink":true,"copy":true,"watch":true}},"created_at":"2026-04-09T10:00:00Z"}}]}}
"#,
            workspace.path().join(".codex-alt/skills").display()
        ),
    );
    write_file(
        &registry.join("bindings.json"),
        &format!(
            r#"{{"schema_version":1,"bindings":[{{"binding_id":"bind_codex_project","agent":"codex","profile_id":"default","workspace_matcher":{{"kind":"path_prefix","value":"{}"}},"default_target_id":"target_codex_project","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"}}]}}
"#,
            workspace.path().display()
        ),
    );
    write_file(
        &registry.join("rules.json"),
        r#"{"schema_version":1,"rules":[{"binding_id":"bind_codex_project","skill_id":"demo","target_id":"target_codex_project","method":"copy","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"},{"binding_id":"bind_codex_project","skill_id":"alt","target_id":"target_codex_alt","method":"copy","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &registry.join("projections.json"),
        r#"{"schema_version":1,"projections":[]}"#,
    );
    write_file(
        &registry.join("ops/checkpoint.json"),
        r#"{"schema_version":1,"last_scanned_op_id":null,"last_acked_op_id":null,"updated_at":"2026-04-09T10:07:00Z"}
"#,
    );
    write_file(&registry.join("ops/operations.jsonl"), "");

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(
        output.status.success(),
        "rule target provision plan failed: {env}"
    );
    let views = env["data"]["plan"]["active_views"].as_array().unwrap();
    let default_view = views
        .iter()
        .find(|view| view["source_target_id"] == "target_codex_project")
        .expect("default target view");
    let alt_view = views
        .iter()
        .find(|view| view["source_target_id"] == "target_codex_alt")
        .expect("alt target view");
    assert_eq!(default_view["skills"], json!(["demo"]));
    assert_eq!(alt_view["skills"], json!(["alt"]));
    assert_eq!(
        alt_view["path"],
        json!(format!(
            "/workspaces/{}/.codex-alt/skills",
            workspace.path().file_name().unwrap().to_string_lossy()
        ))
    );
    let setup = file_plan(
        &env["data"]["plan"]["files_to_write"],
        ".devcontainer/loom-setup.sh",
    );
    assert!(
        setup["preview"]
            .as_str()
            .unwrap()
            .contains(".codex-alt/skills")
    );
    assert!(
        setup["preview"]
            .as_str()
            .unwrap()
            .contains("$ACTIVE_VIEW/alt/SKILL.md")
    );
}

#[test]
fn provision_doctor_reads_plan_artifact_without_regenerating() {
    let root = TestDir::new("provision-doctor-plan-root");
    let workspace = TestDir::new("provision-doctor-plan-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state(&root, &workspace);
    let plan_path = root.path().join("provision-plan.json");
    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let plan_arg = plan_path.to_string_lossy().to_string();
    let (plan_output, plan_env) = run_loom(
        root.path(),
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            &workspace_arg,
            "--output-plan",
            &plan_arg,
        ],
    );
    assert!(
        plan_output.status.success(),
        "provision plan artifact failed: {plan_env}"
    );

    write_file(
        &root.path().join("state/registry/rules.json"),
        r#"{"schema_version":1,"rules":[]}
"#,
    );
    let (doctor_output, doctor_env) = run_loom(
        root.path(),
        &[
            "provision",
            "doctor",
            "--target",
            "devcontainer",
            "--plan",
            &plan_arg,
        ],
    );

    assert!(
        doctor_output.status.success(),
        "provision doctor artifact failed: {doctor_env}"
    );
    assert_eq!(doctor_env["data"]["plan_source"], json!("artifact"));
    assert_eq!(
        doctor_env["data"]["checks"]["adapter_paths"]["active_views"][0]["skills"],
        json!(["demo"])
    );
}

#[test]
fn provision_doctor_is_read_only_and_reports_missing_generated_files() {
    let root = TestDir::new("provision-doctor-root");
    let workspace = TestDir::new("provision-doctor-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state(&root, &workspace);

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "doctor",
            "--target",
            "devcontainer",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(output.status.success(), "provision doctor failed: {env}");
    assert_eq!(env["cmd"], json!("provision.doctor"));
    assert_eq!(env["data"]["status"], json!("action_required"));
    assert_eq!(env["data"]["target_writes_performed"], json!(false));
    assert_eq!(
        env["data"]["checks"]["generated_files"]["files"][0]["status"],
        json!("missing")
    );
    assert!(!workspace.path().join(".devcontainer").exists());
}

#[test]
fn provision_apply_is_deferred_by_policy_gate() {
    let root = TestDir::new("provision-apply-root");

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            "provplan_demo",
            "--idempotency-key",
            "key-123",
        ],
    );

    assert!(!output.status.success(), "provision apply should fail");
    assert_eq!(env["cmd"], json!("provision.apply"));
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
}
