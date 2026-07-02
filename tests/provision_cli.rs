mod common;

use std::process::Command;

use common::{TestDir, run_loom, write_file, write_skill};
use serde_json::{Value, json};

fn write_codex_registry_state(root: &TestDir, workspace: &TestDir) {
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
            workspace.path().display()
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
    assert!(!setup["preview"].as_str().unwrap().contains("token@"));
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
