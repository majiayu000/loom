mod common;

use std::fs;
use std::process::{Command, Stdio};

use common::{TestDir, run_loom, run_loom_with_env, write_file};
use serde_json::Value;

fn adapter_by_id<'a>(env: &'a Value, id: &str) -> &'a Value {
    env["data"]["agent_adapters"]["adapters"]
        .as_array()
        .expect("adapters")
        .iter()
        .find(|adapter| adapter["id"] == id)
        .expect("adapter")
}

fn array_contains(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .expect("array")
        .iter()
        .any(|item| item.as_str() == Some(expected))
}

fn run_loom_with_removed_home(
    root: &std::path::Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> (std::process::Output, Value) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_loom"));
    cmd.arg("--json").arg("--root").arg(root).args(args);
    cmd.env_remove("HOME").env_remove("USERPROFILE");
    for (key, value) in envs {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

// Exercises the reentrant-lock production path: cmd_workspace_init holds the
// workspace lock at line 159 and then calls cmd_target_add (line 200) which
// re-acquires the same lock.  If reentrancy is broken this hangs or panics.
#[test]
fn workspace_init_scan_existing_imports_present_dirs() {
    let root = TestDir::new("ws-init-scan-import");
    let fake_home = TestDir::new("ws-init-scan-import-home");

    fs::create_dir_all(fake_home.path().join(".claude/skills")).expect("create .claude/skills");
    fs::create_dir_all(fake_home.path().join(".codex/skills")).expect("create .codex/skills");
    fs::create_dir_all(fake_home.path().join(".cursor/skills")).expect("create .cursor/skills");

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(
        output.status.success(),
        "workspace init --scan-existing failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["meta"].get("op_id"), None);
    assert_eq!(env["data"]["initialized"], Value::Bool(true));
    assert_eq!(env["data"]["scanned"], Value::Bool(true));
    assert_eq!(
        env["data"]["imported"].as_array().map(|a| a.len()),
        Some(3),
        "expected present default dirs imported: {:?}",
        env["data"]
    );
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(7));

    // Confirm the targets are actually persisted.
    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success());
    assert_eq!(list_env["data"]["count"], Value::from(3));
}

#[test]
fn workspace_init_scan_existing_skips_absent_dirs() {
    let root = TestDir::new("ws-init-scan-skip");
    let fake_home = TestDir::new("ws-init-scan-skip-home");

    // Only create the Claude dir; all other default agent dirs intentionally absent.
    fs::create_dir_all(fake_home.path().join(".claude/skills")).expect("create .claude/skills");

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["data"]["imported"].as_array().map(|a| a.len()), Some(1));
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(9));
    assert_eq!(
        env["data"]["skipped"][0]["reason"],
        Value::String("does-not-exist".to_string())
    );
}

#[test]
fn workspace_init_scan_existing_loads_external_adapter_fixture() {
    let root = TestDir::new("ws-init-external-adapter");
    let fake_home = TestDir::new("ws-init-external-adapter-home");
    let external_dir = root.path().join("fixture-agent/skills");
    fs::create_dir_all(&external_dir).expect("create external skill dir");
    write_file(
        &root.path().join("adapters/fixture-agent.json"),
        &format!(
            r#"{{
  "adapter_api": "1",
  "id": "fixture-agent",
  "supported_scopes": ["project"],
  "projection_methods": ["copy", "symlink"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {{
    "automatic_discovery": true,
    "explicit_invocation": true,
    "reload_required": true
  }},
  "default_skill_dirs": ["{}"]
}}
"#,
            external_dir.display()
        ),
    );

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );
    assert!(
        output.status.success(),
        "workspace init with external adapter failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["data"]["imported"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        env["data"]["imported"][0]["target"]["agent"],
        "fixture-agent"
    );
    assert_eq!(
        env["data"]["imported"][0]["target"]["agent_source"],
        "external"
    );

    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success());
    assert_eq!(list_env["data"]["count"], Value::from(1));
    assert_eq!(list_env["data"]["targets"][0]["agent"], "fixture-agent");
    assert_eq!(list_env["data"]["targets"][0]["agent_source"], "external");
}

#[test]
fn workspace_init_loads_external_v2_adapter_fixture() {
    let root = TestDir::new("ws-init-external-adapter-v2");
    let fake_home = TestDir::new("ws-init-external-adapter-v2-home");
    let external_dir = root.path().join("fixture-v2/skills");
    fs::create_dir_all(&external_dir).expect("create external skill dir");
    write_file(
        &root.path().join("adapters/fixture-v2.json"),
        &format!(
            r#"{{
  "adapter_api": "2",
  "id": "fixture-v2",
  "supported_scopes": ["user", "project"],
  "projection_methods": ["copy", "symlink"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {{
    "automatic_discovery": true,
    "explicit_invocation": true,
    "reload_required": true
  }},
  "discovery_roots": [
    {{
      "scope": "user",
      "path": "{}",
      "role": "preferred-cross-client"
    }},
    {{
      "scope": "project",
      "path": "<workspace>/.fixture-v2/skills",
      "role": "project-cross-client",
      "scan_eligible": false
    }}
  ],
  "visibility": {{
    "follows_symlink_dirs": true,
    "identity_by_projection_method": {{
      "symlink": "canonical-skill-md-path",
      "copy": "runtime-skill-md-path"
    }},
    "disable_rules": ["adapter-defined"]
  }},
  "reload": {{
    "strategy": "restart-required",
    "hot_reload": false
  }}
}}
"#,
            external_dir.display()
        ),
    );

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(
        output.status.success(),
        "workspace init with external v2 adapter failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["data"]["imported"].as_array().map(Vec::len), Some(1));
    assert_eq!(env["data"]["imported"][0]["target"]["agent"], "fixture-v2");

    let (status_output, status_env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(status_output.status.success());
    let adapter = adapter_by_id(&status_env, "fixture-v2");
    assert_eq!(adapter["declared_adapter_api"], "2");
    assert_eq!(adapter["discovery_roots"].as_array().map(Vec::len), Some(2));
    assert_eq!(adapter["reload"]["strategy"], "restart-required");
}

#[test]
fn workspace_status_reports_codex_v2_adapter_metadata() {
    let root = TestDir::new("ws-status-codex-v2");
    let fake_home = TestDir::new("ws-status-codex-v2-home");
    let home_str = fake_home.path().to_string_lossy().into_owned();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "status"],
    );

    assert!(
        output.status.success(),
        "workspace status failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let adapter = adapter_by_id(&env, "codex");
    assert_eq!(adapter["declared_adapter_api"], "2");
    assert!(
        adapter["discovery_roots"]
            .as_array()
            .expect("roots")
            .iter()
            .any(|root| root["scope"] == "user"
                && root["role"] == "preferred-cross-client"
                && root["path_template"] == "~/.agents/skills")
    );
    assert!(
        adapter["discovery_roots"]
            .as_array()
            .expect("roots")
            .iter()
            .any(|root| root["scope"] == "project"
                && root["role"] == "project-cross-client"
                && root["path_template"] == "<workspace>/.agents/skills")
    );
    assert_eq!(adapter["reload"]["strategy"], "new-session-recommended");
    assert!(array_contains(
        &adapter["visibility"]["disable_rules"],
        "skills.config.path"
    ));
}

#[test]
fn workspace_init_rejects_v1_adapter_unknown_fields() {
    let root = TestDir::new("ws-init-v1-adapter-unknown-field");
    let fake_home = TestDir::new("ws-init-v1-adapter-unknown-field-home");
    write_file(
        &root.path().join("adapters/bad-v1.json"),
        r#"{
  "adapter_api": "1",
  "id": "bad-v1",
  "supported_scopes": ["project"],
  "projection_methods": ["copy"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {
    "automatic_discovery": false,
    "explicit_invocation": true,
    "reload_required": false
  },
  "unexpected": true
}
"#,
    );

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(!output.status.success());
    assert_eq!(env["error"]["code"], "ADAPTER_INVALID");
    assert_eq!(env["error"]["details"]["reason"], "ADAPTER_JSON_INVALID");
}

#[test]
fn workspace_init_scan_existing_invalid_adapter_fails_without_registry_write() {
    let root = TestDir::new("ws-init-invalid-adapter");
    let fake_home = TestDir::new("ws-init-invalid-adapter-home");
    write_file(
        &root.path().join("adapters/bad.json"),
        r#"{
  "adapter_api": "9",
  "id": "bad-agent",
  "supported_scopes": ["project"],
  "projection_methods": ["copy"],
  "skill_entrypoint": "SKILL.md",
  "capabilities": {
    "automatic_discovery": true,
    "explicit_invocation": true,
    "reload_required": false
  },
  "default_skill_dirs": ["/tmp/bad-agent/skills"]
}
"#,
    );

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &["workspace", "init", "--scan-existing"],
    );
    assert!(
        !output.status.success(),
        "invalid adapter should fail: stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["error"]["code"], "ADAPTER_INVALID");
    assert_eq!(env["error"]["details"]["reason"], "ADAPTER_API_UNSUPPORTED");
    assert!(
        !root.path().join("state/registry/schema.json").exists(),
        "invalid adapter must fail before registry layout writes"
    );
}

#[test]
fn top_level_init_with_explicit_root_without_home_initializes_without_scan() {
    let root = TestDir::new("ws-init-no-home");

    let (output, env) = run_loom_with_removed_home(root.path(), &[], &["init"]);

    assert!(
        output.status.success(),
        "top-level init without HOME failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["initialized"], Value::Bool(true));
    assert_eq!(env["data"]["scanned"], Value::Bool(false));
    assert_eq!(env["data"]["imported"].as_array().map(|a| a.len()), Some(0));
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(0));

    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(list_output.status.success());
    assert_eq!(list_env["data"]["count"], Value::from(0));
}

#[test]
fn workspace_init_scan_existing_uses_userprofile_when_home_is_missing() {
    let root = TestDir::new("ws-init-userprofile");
    let fake_profile = TestDir::new("ws-init-userprofile-home");

    fs::create_dir_all(fake_profile.path().join(".claude/skills")).expect("create .claude/skills");
    fs::create_dir_all(fake_profile.path().join(".codex/skills")).expect("create .codex/skills");

    let profile_str = fake_profile.path().to_string_lossy().into_owned();
    let (output, env) = run_loom_with_removed_home(
        root.path(),
        &[("USERPROFILE", &profile_str)],
        &["workspace", "init", "--scan-existing"],
    );

    assert!(
        output.status.success(),
        "workspace init --scan-existing with USERPROFILE failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["scanned"], Value::Bool(true));
    assert_eq!(env["data"]["imported"].as_array().map(|a| a.len()), Some(2));
    assert_eq!(env["data"]["skipped"].as_array().map(|a| a.len()), Some(8));
}

// Two processes race to `workspace init --scan-existing` on the same root.
// The second process will get LOCK_BUSY (the filesystem lock is non-blocking).
// After both finish the state must not be corrupted: exactly two targets should
// exist (idempotency + reentrancy both hold).
#[test]
fn workspace_init_scan_existing_concurrent_inits_leave_consistent_state() {
    let root = TestDir::new("ws-init-concurrent");
    let fake_home = TestDir::new("ws-init-concurrent-home");

    fs::create_dir_all(fake_home.path().join(".claude/skills")).expect("create .claude/skills");
    fs::create_dir_all(fake_home.path().join(".codex/skills")).expect("create .codex/skills");

    let home_str = fake_home.path().to_string_lossy().into_owned();
    let root_str = root.path().to_string_lossy().into_owned();

    let child1 = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(&root_str)
        .args(["workspace", "init", "--scan-existing"])
        .env("HOME", &home_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first loom process");

    let child2 = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(&root_str)
        .args(["workspace", "init", "--scan-existing"])
        .env("HOME", &home_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn second loom process");

    let out1 = child1.wait_with_output().expect("wait for first process");
    let out2 = child2.wait_with_output().expect("wait for second process");

    // At least one must succeed; the other may get LOCK_BUSY.
    assert!(
        out1.status.success() || out2.status.success(),
        "neither concurrent init succeeded: stderr1={} stderr2={}",
        String::from_utf8_lossy(&out1.stderr),
        String::from_utf8_lossy(&out2.stderr)
    );

    // State must be consistent regardless of which process won the race.
    let (list_output, list_env) = run_loom(root.path(), &["target", "list"]);
    assert!(
        list_output.status.success(),
        "target list failed after concurrent inits: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    assert_eq!(
        list_env["data"]["count"],
        Value::from(2),
        "expected exactly 2 targets after concurrent inits"
    );
}
