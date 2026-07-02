mod common;

use std::fs;
use std::path::Path;
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

fn seed_git(root: &TestDir) {
    let init = Command::new("git")
        .arg("init")
        .arg(root.path())
        .output()
        .expect("git init");
    assert!(init.status.success(), "git init failed");
    commit_provision_registry(root, "seed registry");
}

fn commit_provision_registry(root: &TestDir, message: &str) {
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
            message,
        ])
        .output()
        .expect("git commit");
    assert!(
        commit.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );
}

fn build_plan(root: &TestDir, workspace: &TestDir, plan_path: &Path) -> Value {
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Demo.\n---\n# Demo\n",
    );
    write_codex_registry_state(root, workspace);
    seed_git(root);

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let plan_arg = plan_path.to_string_lossy().to_string();
    let (output, env) = run_loom(
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

    assert!(output.status.success(), "provision plan failed: {env}");
    assert!(plan_path.is_file(), "plan artifact should be written");
    env
}

#[test]
fn provision_apply_writes_reviewed_files_and_replays_idempotently() {
    let root = TestDir::new("provision-apply-root");
    let workspace = TestDir::new("provision-apply-workspace");
    let plan_path = root.path().join("plan.json");
    let plan_env = build_plan(&root, &workspace, &plan_path);
    let plan_arg = plan_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            &plan_arg,
            "--idempotency-key",
            "key-123",
            "--approve",
            "approval:provision-apply",
        ],
    );

    assert!(output.status.success(), "provision apply failed: {env}");
    assert_eq!(env["cmd"], json!("provision.apply"));
    assert_eq!(env["data"]["plan_id"], plan_env["data"]["plan"]["plan_id"]);
    assert_eq!(env["data"]["target_writes_performed"], json!(true));
    assert_eq!(env["data"]["idempotent_replay"], json!(false));
    assert!(
        env["data"]["idempotency_key_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert!(!serde_json::to_string(&env).unwrap().contains("key-123"));
    assert!(
        workspace
            .path()
            .join(".devcontainer/loom-setup.sh")
            .is_file()
    );
    assert!(
        workspace
            .path()
            .join(".devcontainer/devcontainer.json")
            .is_file()
    );
    assert!(
        env["data"]["written_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == ".devcontainer/loom-setup.sh")
    );
    assert!(
        env["data"]["recovery"]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cmd| cmd
                .as_str()
                .unwrap()
                .contains(".devcontainer/loom-setup.sh"))
    );

    let (replay_output, replay_env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            &plan_arg,
            "--idempotency-key",
            "key-123",
            "--approve",
            "approval:provision-apply",
        ],
    );

    assert!(
        replay_output.status.success(),
        "provision apply replay failed: {replay_env}"
    );
    assert_eq!(replay_env["data"]["idempotent_replay"], json!(true));
    assert_eq!(replay_env["data"]["target_writes_performed"], json!(false));
}

#[test]
fn provision_apply_requires_reviewed_approval_token() {
    let root = TestDir::new("provision-apply-approval-root");
    let workspace = TestDir::new("provision-apply-approval-workspace");
    let plan_path = root.path().join("plan.json");
    build_plan(&root, &workspace, &plan_path);
    let plan_arg = plan_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            &plan_arg,
            "--idempotency-key",
            "key-approval",
        ],
    );

    assert!(
        !output.status.success(),
        "apply without approval should fail"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        env["error"]["details"]["missing_approvals"],
        json!(["approval:provision-apply"])
    );
    assert_eq!(
        env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert!(!workspace.path().join(".devcontainer").exists());
}

#[test]
fn provision_apply_rejects_preimage_drift_without_overwrite() {
    let root = TestDir::new("provision-apply-drift-root");
    let workspace = TestDir::new("provision-apply-drift-workspace");
    let plan_path = root.path().join("plan.json");
    build_plan(&root, &workspace, &plan_path);
    let devcontainer = workspace.path().join(".devcontainer/devcontainer.json");
    write_file(&devcontainer, "{\"name\":\"manual\"}\n");
    let plan_arg = plan_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            &plan_arg,
            "--idempotency-key",
            "key-drift",
            "--approve",
            "approval:provision-apply",
        ],
    );

    assert!(!output.status.success(), "drifted apply should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert_eq!(
        fs::read_to_string(devcontainer).expect("read drifted file"),
        "{\"name\":\"manual\"}\n"
    );
}

#[test]
fn provision_apply_rejects_stale_registry_head() {
    let root = TestDir::new("provision-apply-stale-root");
    let workspace = TestDir::new("provision-apply-stale-workspace");
    let plan_path = root.path().join("plan.json");
    build_plan(&root, &workspace, &plan_path);
    write_file(
        &root.path().join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Demo changed.\n---\n# Demo Changed\n",
    );
    commit_provision_registry(&root, "change registry after plan");
    let plan_arg = plan_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            &plan_arg,
            "--idempotency-key",
            "key-stale",
            "--approve",
            "approval:provision-apply",
        ],
    );

    assert!(!output.status.success(), "stale registry apply should fail");
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("registry head is stale")
    );
    assert_eq!(
        env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert!(!workspace.path().join(".devcontainer").exists());
}

#[test]
fn provision_apply_rejects_credential_bearing_clone_url_artifact() {
    let root = TestDir::new("provision-apply-url-root");
    let workspace = TestDir::new("provision-apply-url-workspace");
    let plan_path = root.path().join("plan.json");
    build_plan(&root, &workspace, &plan_path);
    let mut plan: Value =
        serde_json::from_str(&fs::read_to_string(&plan_path).expect("read plan artifact"))
            .expect("parse plan artifact");
    plan["registry_clone_url"] = json!("https://token@example.com/org/loom-registry.git");
    plan["registry_source_display"] = json!("https://token@example.com/org/loom-registry.git");
    write_file(
        &plan_path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&plan).expect("serialize plan artifact")
        ),
    );
    let plan_arg = plan_path.to_string_lossy().to_string();

    let (output, env) = run_loom(
        root.path(),
        &[
            "provision",
            "apply",
            &plan_arg,
            "--idempotency-key",
            "key-url",
            "--approve",
            "approval:provision-apply",
        ],
    );

    assert!(
        !output.status.success(),
        "credential-bearing clone URL should fail"
    );
    assert_eq!(env["error"]["code"], json!("POLICY_BLOCKED"));
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("credential-redacted registry clone URL")
    );
    assert_eq!(
        env["error"]["details"]["target_writes_performed"],
        json!(false)
    );
    assert!(!workspace.path().join(".devcontainer").exists());
}
