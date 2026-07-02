mod common;

use std::path::Path;
use std::process::Command;

use common::{TestDir, run_loom, run_loom_with_env, write_file, write_skill};
use serde_json::{Value, json};

fn write_single_skill_state(root: &TestDir, workspace: &Path, skill: &str) {
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
        &format!(
            r#"{{"schema_version":1,"rules":[{{"binding_id":"bind_codex_project","skill_id":"{}","target_id":"target_codex_project","method":"copy","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"}}]}}
"#,
            skill
        ),
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
}

fn setup_preview<'a>(plan: &'a Value, path: &str) -> &'a str {
    plan["files_to_write"]
        .as_array()
        .expect("files array")
        .iter()
        .find(|file| file["path"] == path)
        .expect("matching file plan")["preview"]
        .as_str()
        .expect("preview string")
}

#[test]
fn provision_plan_redacts_ssh_password_and_marks_remote_secrets_unresolved() {
    let root = TestDir::new("provision-ssh-secret-root");
    let workspace = TestDir::new("provision-ssh-secret-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_file(
        &root.path().join("skills/demo/loom.skill.toml"),
        "requires_env = [\"DEMO_TOKEN\"]\n",
    );
    write_single_skill_state(&root, workspace.path(), "demo");
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
            "ssh://user:password@example.com/org/loom-registry.git",
        ])
        .output()
        .expect("git remote add");

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("DEMO_TOKEN", "local-secret")],
        &[
            "provision",
            "plan",
            "--target",
            "devcontainer",
            "--workspace",
            &workspace_arg,
        ],
    );

    assert!(output.status.success(), "provision plan failed: {env}");
    let plan = &env["data"]["plan"];
    assert_eq!(
        plan["registry_clone_url"],
        json!("ssh://user@example.com/org/loom-registry.git")
    );
    let serialized = serde_json::to_string(plan).unwrap();
    assert!(!serialized.contains("password"));
    assert!(!serialized.contains("local-secret"));
    assert!(
        plan["secrets_required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|secret| secret["name"] == "GIT_CREDENTIALS")
    );
    let demo_secret = plan["secrets_required"]
        .as_array()
        .unwrap()
        .iter()
        .find(|secret| secret["name"] == "DEMO_TOKEN")
        .expect("demo secret");
    assert_eq!(demo_secret["present"], json!(false));
}

#[test]
fn provision_plan_reports_missing_active_skill_without_aborting() {
    let root = TestDir::new("provision-missing-skill-root");
    let workspace = TestDir::new("provision-missing-skill-workspace");
    write_single_skill_state(&root, workspace.path(), "demo");

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
        "missing skill should be diagnosed: {env}"
    );
    let readiness = &env["data"]["plan"]["dependency_readiness"][0];
    assert_eq!(readiness["skill"], json!("demo"));
    assert_eq!(readiness["status"], json!("missing"));
    assert_eq!(readiness["ready"], json!(false));
    assert!(
        env["data"]["plan"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["id"] == "active_skill_missing")
    );
}

#[test]
fn provision_doctor_blocks_on_safety_findings_even_when_files_are_present() {
    let root = TestDir::new("provision-safety-root");
    let workspace = TestDir::new("provision-safety-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_single_skill_state(&root, workspace.path(), "demo");
    write_file(
        &root.path().join("state/registry/trust.json"),
        r#"{"schema_version":1,"skills":[{"skill_id":"demo","trust":"blocked","quarantined":false,"reason":"test","updated_at":"2026-04-09T10:00:00Z","updated_by":"test"}]}
"#,
    );

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (plan_output, plan_env) = run_loom(
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
        plan_output.status.success(),
        "safety provision plan failed: {plan_env}"
    );
    let plan = &plan_env["data"]["plan"];
    assert!(
        plan["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["id"] == "skill_safety_policy_blocked")
    );
    for file in plan["files_to_write"].as_array().unwrap() {
        write_file(
            &workspace.path().join(file["path"].as_str().unwrap()),
            file["preview"].as_str().unwrap(),
        );
    }

    let (doctor_output, doctor_env) = run_loom(
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

    assert!(
        doctor_output.status.success(),
        "safety provision doctor failed: {doctor_env}"
    );
    assert_eq!(
        doctor_env["data"]["checks"]["generated_files"]["status"],
        json!("pass")
    );
    assert_eq!(
        doctor_env["data"]["checks"]["findings"]["status"],
        json!("warning")
    );
    assert_eq!(doctor_env["data"]["status"], json!("action_required"));
    assert!(!setup_preview(plan, ".devcontainer/loom-setup.sh").is_empty());
}
