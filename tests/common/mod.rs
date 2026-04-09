#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use uuid::Uuid;

pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(prefix: &str) -> Self {
        let path = std::env::temp_dir().join(format!("loom-{}-{}", prefix, Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn run_loom(root: &Path, args: &[&str]) -> (Output, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--json")
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run loom");
    let env = serde_json::from_slice(&output.stdout).expect("parse loom json");
    (output, env)
}

pub fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, body).expect("write file");
}

pub fn write_skill(root: &Path, skill: &str, body: &str) {
    write_file(&root.join("skills").join(skill).join("SKILL.md"), body);
}

pub fn write_minimal_v3_state(root: &Path, schema_version: u32) {
    let v3 = root.join("state/v3");
    write_file(
        &v3.join("schema.json"),
        &format!(
            "{{\"schema_version\":{},\"created_at\":\"2026-04-09T10:00:00Z\",\"writer\":\"loom/3.0.0-draft\"}}\n",
            schema_version
        ),
    );
    write_file(
        &v3.join("targets.json"),
        r#"{"schema_version":3,"targets":[{"target_id":"target_claude_project_a","agent":"claude","path":"/tmp/claude-a/skills","ownership":"managed","capabilities":{"symlink":true,"copy":true,"watch":true},"created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &v3.join("bindings.json"),
        r#"{"schema_version":3,"bindings":[{"binding_id":"bind_claude_project_a","agent":"claude","profile_id":"default","workspace_matcher":{"kind":"path_prefix","value":"/tmp/project-a"},"default_target_id":"target_claude_project_a","policy_profile":"safe-capture","active":true,"created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &v3.join("rules.json"),
        r#"{"schema_version":3,"rules":[{"binding_id":"bind_claude_project_a","skill_id":"model-onboarding","target_id":"target_claude_project_a","method":"symlink","watch_policy":"observe_only","created_at":"2026-04-09T10:00:00Z"}]}
"#,
    );
    write_file(
        &v3.join("projections.json"),
        r#"{"schema_version":3,"projections":[{"instance_id":"inst_model-onboarding_claude_a","skill_id":"model-onboarding","binding_id":"bind_claude_project_a","target_id":"target_claude_project_a","materialized_path":"/tmp/claude-a/skills/model-onboarding","method":"symlink","last_applied_rev":"abc123","health":"healthy","observed_drift":false,"updated_at":"2026-04-09T10:05:00Z"}]}
"#,
    );
    write_file(
        &v3.join("ops/checkpoint.json"),
        r#"{"schema_version":3,"last_scanned_op_id":"op_001","last_acked_op_id":null,"updated_at":"2026-04-09T10:07:00Z"}
"#,
    );
    write_file(
        &v3.join("ops/operations.jsonl"),
        r#"{"op_id":"op_001","intent":"skill.project","status":"succeeded","ack":false,"payload":{"skill_id":"model-onboarding","binding_id":"bind_claude_project_a"},"effects":{"instance_id":"inst_model-onboarding_claude_a"},"created_at":"2026-04-09T10:05:00Z","updated_at":"2026-04-09T10:05:00Z"}
"#,
    );
}

pub fn write_legacy_targets(root: &Path, payload: Value) {
    let state_dir = root.join("state");
    fs::create_dir_all(&state_dir).expect("create state dir");
    fs::write(
        state_dir.join("targets.json"),
        serde_json::to_string_pretty(&payload).expect("serialize legacy targets"),
    )
    .expect("write legacy targets");
}

pub fn legacy_target_payload(
    method: &str,
    claude_path: Option<String>,
    codex_path: Option<String>,
) -> Value {
    json!({
        "skills": {
            "demo": {
                "method": method,
                "claude_path": claude_path,
                "codex_path": codex_path
            }
        }
    })
}
