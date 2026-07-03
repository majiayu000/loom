mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_file, write_minimal_registry_state, write_skill};

fn good_skill_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Use when an agent needs to inspect one skill lifecycle status.\n---\n# {name}\n"
    )
}

fn write_good_skill(root: &Path, skill: &str) {
    write_skill(root, skill, &good_skill_body(skill));
}

fn setup_projected_skill() -> (TestDir, String) {
    let root = TestDir::new("skill-inspect-projected");
    write_good_skill(root.path(), "model-onboarding");
    assert!(
        save_skill(root.path(), "model-onboarding")
            .0
            .status
            .success(),
        "save skill should pass"
    );

    let target_path = root.path().join("live/claude-project-a");
    let (target_output, target_env) = target_add(root.path(), "claude", &target_path, "managed");
    assert!(
        target_output.status.success(),
        "target add should pass: {target_env}"
    );
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id")
        .to_string();

    let (binding_output, binding_env) = binding_add(
        root.path(),
        "claude",
        "default",
        "path-prefix",
        "/tmp/project-a",
        &target_id,
    );
    assert!(
        binding_output.status.success(),
        "binding add should pass: {binding_env}"
    );

    let (project_output, project_env) = skill_project(
        root.path(),
        "model-onboarding",
        "bind_claude_project_a",
        None,
    );
    assert!(
        project_output.status.success(),
        "project should pass: {project_env}"
    );
    (root, target_id)
}

fn tree_snapshot(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    if path.exists() {
        collect_files(path, path, &mut out);
    }
    out
}

fn collect_files(root: &Path, path: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    if path.is_file() {
        let rel = path
            .strip_prefix(root)
            .expect("path under root")
            .to_string_lossy()
            .to_string();
        out.insert(rel, fs::read(path).expect("read snapshot file"));
        return;
    }
    let mut entries = fs::read_dir(path)
        .expect("read snapshot dir")
        .collect::<Result<Vec<_>, _>>()
        .expect("read snapshot entries");
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        collect_files(root, &entry.path(), out);
    }
}

fn has_runtime_finding(runtime: &Value, id: &str) -> bool {
    runtime["findings"]
        .as_array()
        .expect("runtime findings")
        .iter()
        .any(|finding| finding["id"] == Value::String(id.to_string()))
}

fn git_init(path: &Path) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("init")
        .output()
        .expect("run git init");
    assert!(
        output.status.success(),
        "git init failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn skill_inspect_missing_skill_returns_typed_error() {
    let root = TestDir::new("skill-inspect-missing");

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "ghost"]);

    assert!(!output.status.success(), "inspect should fail: {env}");
    assert_eq!(env["cmd"], Value::String("skill.inspect".to_string()));
    assert_eq!(
        env["error"]["code"],
        Value::String("SKILL_NOT_FOUND".to_string())
    );
}

#[test]
fn skill_inspect_source_only_reports_registry_install_without_projection() {
    let root = TestDir::new("skill-inspect-source-only");
    write_good_skill(root.path(), "demo");

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "demo"]);

    assert!(output.status.success(), "inspect should pass: {env}");
    assert_eq!(env["cmd"], Value::String("skill.inspect".to_string()));
    assert_eq!(env["data"]["skill"], Value::String("demo".to_string()));
    assert_eq!(env["data"]["source"]["exists"], Value::Bool(true));
    assert_eq!(
        env["data"]["source"]["entrypoint_exists"],
        Value::Bool(true)
    );
    assert_eq!(
        env["data"]["spec"]["portable"],
        Value::String("pass".to_string())
    );
    assert_eq!(
        env["data"]["runtime"]["codex"]["installed_in_registry"],
        Value::Bool(true)
    );
    assert_eq!(
        env["data"]["runtime"]["codex"]["active_rule_present"],
        Value::Bool(false)
    );
    assert_eq!(
        env["data"]["runtime"]["codex"]["projected_to_target"],
        Value::Bool(false)
    );
    assert_eq!(
        env["data"]["runtime"]["codex"]["visible_to_agent"],
        Value::String("not_checked".to_string())
    );
    assert!(
        env["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action
                .as_str()
                .is_some_and(|text| text.contains("skill eval demo"))),
        "missing eval evidence should produce an eval action: {env}"
    );
}

#[test]
fn skill_inspect_reports_compiled_artifact_summary() {
    let root = TestDir::new("skill-inspect-compiled-summary");
    write_good_skill(root.path(), "demo");
    let (_output, compile) = run_loom(
        root.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );
    assert_eq!(compile["ok"], json!(true), "{compile}");
    let artifact_id = compile["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_string();

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "demo"]);

    assert!(output.status.success(), "inspect should pass: {env}");
    assert_eq!(env["data"]["compiled"]["skill"], json!("demo"));
    assert_eq!(env["data"]["compiled"]["count"], json!(1));
    assert_eq!(
        env["data"]["compiled"]["artifacts"][0]["artifact_id"],
        json!(artifact_id)
    );
    assert_eq!(
        env["data"]["compiled"]["artifacts"][0]["manifest_status"],
        json!("parseable")
    );
    assert_eq!(
        env["data"]["compiled"]["artifacts"][0]["status"],
        json!("experimental")
    );
    assert_eq!(
        env["data"]["compiled"]["artifacts"][0]["agent"],
        json!("codex")
    );
}

#[test]
fn skill_inspect_handles_unborn_git_repo_without_head() {
    let root = TestDir::new("skill-inspect-unborn-git");
    write_good_skill(root.path(), "demo");
    git_init(root.path());

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "demo"]);

    assert!(output.status.success(), "inspect should pass: {env}");
    assert_eq!(
        env["data"]["source"]["working_tree_drift"],
        Value::Bool(false)
    );
    assert_eq!(env["data"]["source"]["last_source_commit"], Value::Null);
}

#[test]
fn skill_inspect_populates_recorded_provenance() {
    let root = TestDir::new("skill-inspect-provenance");
    let source = TestDir::new("skill-inspect-provenance-source");
    write_file(&source.path().join("SKILL.md"), &good_skill_body("demo"));
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(root.path(), &["skill", "add", source_arg, "--name", "demo"]);
    assert!(output.status.success(), "skill add should pass: {env}");

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "demo"]);

    assert!(output.status.success(), "inspect should pass: {env}");
    assert_eq!(env["data"]["provenance"]["source"], json!(source_arg));
    assert_eq!(env["data"]["provenance"]["verified"], Value::Bool(true));
    assert_eq!(env["data"]["provenance"]["drift"], Value::Bool(false));
}

#[test]
fn skill_inspect_human_output_prints_compact_card() {
    let root = TestDir::new("skill-inspect-human");
    write_good_skill(root.path(), "demo");

    let output = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("--root")
        .arg(root.path())
        .args(["skill", "inspect", "demo"])
        .output()
        .expect("run loom skill inspect");

    assert!(
        output.status.success(),
        "inspect unexpectedly failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("demo\n"),
        "missing skill card title: {stdout}"
    );
    assert!(stdout.contains("Source:   present, clean"));
    assert!(stdout.contains("Runtime:  "));
    assert!(stdout.contains("Next:     "));
    assert!(
        !stdout.contains("\"source\"") && !stdout.contains("\"runtime\""),
        "human output should not fall back to raw JSON: {stdout}"
    );
    assert!(
        !stdout.contains("skill.inspect ok"),
        "human inspect output should be just the compact card: {stdout}"
    );
}

#[test]
fn skill_inspect_projected_source_separates_rule_projection_and_visibility() {
    let (root, target_id) = setup_projected_skill();

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "inspect",
            "model-onboarding",
            "--agent",
            "claude",
            "--workspace",
            "/tmp/project-a/src",
        ],
    );

    assert!(output.status.success(), "inspect should pass: {env}");
    let runtime = &env["data"]["runtime"];
    assert!(
        runtime.get("codex").is_none(),
        "--agent must filter runtime sections"
    );
    let claude = &runtime["claude"];
    assert_eq!(claude["target_id"], Value::String(target_id));
    assert_eq!(
        claude["binding_id"],
        Value::String("bind_claude_project_a".to_string())
    );
    assert_eq!(claude["installed_in_registry"], Value::Bool(true));
    assert_eq!(claude["active_rule_present"], Value::Bool(true));
    assert_eq!(claude["projected_to_target"], Value::Bool(true));
    assert_eq!(claude["materialized_path_exists"], Value::Bool(true));
    assert_eq!(
        claude["visible_to_agent"],
        Value::String("unknown".to_string())
    );
    assert_eq!(
        claude["enabled_by_agent_config"],
        Value::String("unknown".to_string())
    );
    assert_eq!(
        claude["truth_level"],
        Value::String("registry_projection".to_string())
    );
    assert!(
        env["data"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action.as_str() == Some("loom skill diagnose model-onboarding")),
        "unknown visibility should produce diagnose action: {env}"
    );
}

#[test]
fn skill_inspect_path_prefix_requires_component_boundary() {
    let (root, _) = setup_projected_skill();

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "inspect",
            "model-onboarding",
            "--agent",
            "claude",
            "--workspace",
            "/tmp/project-a2/src",
        ],
    );

    assert!(output.status.success(), "inspect should pass: {env}");
    let claude = &env["data"]["runtime"]["claude"];
    assert_eq!(claude["active_rule_present"], Value::Bool(false));
    assert_eq!(claude["projected_to_target"], Value::Bool(false));
}

#[test]
fn skill_inspect_reports_missing_materialized_projection_path() {
    let (root, _) = setup_projected_skill();
    fs::remove_file(root.path().join("live/claude-project-a/model-onboarding"))
        .expect("remove projected symlink");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "inspect", "model-onboarding", "--agent", "claude"],
    );

    assert!(output.status.success(), "inspect should pass: {env}");
    let claude = &env["data"]["runtime"]["claude"];
    assert_eq!(claude["projected_to_target"], Value::Bool(true));
    assert_eq!(claude["materialized_path_exists"], Value::Bool(false));
    assert!(
        has_runtime_finding(claude, "materialized_path_missing"),
        "missing projection path should be explained: {env}"
    );
}

#[test]
fn skill_inspect_reports_projection_with_missing_target() {
    let (root, _) = setup_projected_skill();
    let targets_path = root.path().join("state/registry/targets.json");
    let mut targets: Value =
        serde_json::from_str(&fs::read_to_string(&targets_path).expect("read targets"))
            .expect("parse targets");
    targets["targets"] = Value::Array(Vec::new());
    fs::write(
        &targets_path,
        serde_json::to_string_pretty(&targets).expect("serialize targets"),
    )
    .expect("write targets");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "inspect", "model-onboarding", "--agent", "claude"],
    );

    assert!(output.status.success(), "inspect should pass: {env}");
    let claude = &env["data"]["runtime"]["claude"];
    assert_eq!(claude["projected_to_target"], Value::Bool(true));
    assert!(
        has_runtime_finding(claude, "target_missing"),
        "missing target should be explained: {env}"
    );
}

#[test]
fn skill_inspect_reports_stale_registry_reference_with_missing_source() {
    let root = TestDir::new("skill-inspect-stale-registry");
    write_minimal_registry_state(root.path(), 1);

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "model-onboarding"]);

    assert!(
        output.status.success(),
        "stale registry reference should return inspect model: {env}"
    );
    assert_eq!(env["data"]["source"]["exists"], Value::Bool(false));
    assert_eq!(
        env["data"]["spec"]["portable"],
        Value::String("error".to_string())
    );
    assert!(
        has_runtime_finding(&env["data"]["runtime"]["claude"], "source_missing"),
        "missing source should be explained: {env}"
    );
}

#[test]
fn skill_inspect_is_read_only() {
    let (root, _) = setup_projected_skill();
    let registry_before = tree_snapshot(&root.path().join("state/registry"));
    let events_before = tree_snapshot(&root.path().join("state/events"));
    let skills_before = tree_snapshot(&root.path().join("skills"));
    let live_before = tree_snapshot(&root.path().join("live"));

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "model-onboarding"]);
    assert!(output.status.success(), "inspect should pass: {env}");

    assert_eq!(
        tree_snapshot(&root.path().join("state/registry")),
        registry_before,
        "inspect must not mutate registry state"
    );
    assert_eq!(
        tree_snapshot(&root.path().join("state/events")),
        events_before,
        "inspect must not write command audit events"
    );
    assert_eq!(
        tree_snapshot(&root.path().join("skills")),
        skills_before,
        "inspect must not mutate skill source"
    );
    assert_eq!(
        tree_snapshot(&root.path().join("live")),
        live_before,
        "inspect must not mutate live target"
    );
}

#[cfg(unix)]
#[test]
fn skill_inspect_reports_broken_symlink_projection() {
    let (root, _) = setup_projected_skill();
    fs::remove_dir_all(root.path().join("skills/model-onboarding")).expect("remove source target");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "inspect", "model-onboarding", "--agent", "claude"],
    );

    assert!(output.status.success(), "inspect should pass: {env}");
    let claude = &env["data"]["runtime"]["claude"];
    assert_eq!(claude["materialized_path_exists"], Value::Bool(false));
    assert!(
        has_runtime_finding(claude, "broken_symlink"),
        "dangling symlink should be explained: {env}"
    );
}

#[cfg(unix)]
#[test]
fn skill_inspect_reports_projection_symlink_to_wrong_source() {
    let (root, _) = setup_projected_skill();
    let wrong_source = root.path().join("wrong-source");
    fs::create_dir_all(&wrong_source).expect("create wrong source");
    fs::write(
        wrong_source.join("SKILL.md"),
        good_skill_body("wrong-source"),
    )
    .expect("write wrong source");
    let projection = root.path().join("live/claude-project-a/model-onboarding");
    fs::remove_file(&projection).expect("remove original projection");
    std::os::unix::fs::symlink(&wrong_source, &projection).expect("create wrong symlink");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "inspect", "model-onboarding", "--agent", "claude"],
    );

    assert!(output.status.success(), "inspect should pass: {env}");
    let claude = &env["data"]["runtime"]["claude"];
    assert_eq!(claude["materialized_path_exists"], Value::Bool(true));
    assert!(
        has_runtime_finding(claude, "projection_source_mismatch"),
        "wrong symlink target should be explained: {env}"
    );
}
