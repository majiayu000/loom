use std::fs;
use std::path::Path;

use serde_json::Value;

mod common;

use common::{TestDir, run_loom_with_env, write_skill};

fn write_good_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when testing activation behavior.\n---\n# {skill}\n"
        ),
    );
}

fn write_compile_ready_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when testing compiled activation behavior.\n---\n# {skill}\n\nUse when testing compiled activation behavior.\n\nDo not use for production claims.\n"
        ),
    );
}

fn run_with_home(root: &Path, home: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let home = home.to_string_lossy().to_string();
    run_loom_with_env(root, &[("HOME", &home)], args)
}

#[test]
fn skill_activate_dry_run_does_not_initialize_registry_or_target() {
    let root = TestDir::new("skill-activate-dry-run");
    let home = TestDir::new("skill-activate-dry-run-home");
    write_good_skill(root.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex", "--dry-run"],
    );

    assert!(
        output.status.success(),
        "dry-run should succeed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(env["data"]["plan"]["dry_run"], Value::Bool(true));
    assert_eq!(
        env["data"]["plan"]["target_path"],
        Value::String(home.path().join(".agents/skills").display().to_string())
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "dry-run must not initialize registry state"
    );
    assert!(
        !home.path().join(".agents/skills/demo").exists(),
        "dry-run must not create target projection"
    );
}

#[test]
fn skill_activate_project_codex_requires_workspace_and_uses_agents_dir() {
    let root = TestDir::new("skill-activate-project");
    let home = TestDir::new("skill-activate-project-home");
    let workspace = TestDir::new("skill-activate-project-workspace");
    write_good_skill(root.path(), "demo");

    let (missing_output, missing_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--scope",
            "project",
            "--dry-run",
        ],
    );
    assert!(
        !missing_output.status.success(),
        "project activation without workspace should fail"
    );
    assert_eq!(
        missing_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );

    let workspace_arg = workspace.path().to_string_lossy().to_string();
    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--scope",
            "project",
            "--workspace",
            &workspace_arg,
            "--dry-run",
        ],
    );
    let expected_target = workspace
        .path()
        .canonicalize()
        .expect("canonical workspace path")
        .join(".agents/skills");
    assert!(
        output.status.success(),
        "project dry-run should pass: {env}"
    );
    assert_eq!(
        env["data"]["plan"]["target_path"],
        Value::String(expected_target.display().to_string())
    );
    assert!(
        !workspace.path().join(".agents/skills/demo").exists(),
        "project dry-run must not create target projection"
    );
}

#[test]
fn skill_activate_artifact_requires_compiled_flag() {
    let root = TestDir::new("skill-activate-artifact-without-compiled");
    let home = TestDir::new("skill-activate-artifact-without-compiled-home");
    write_good_skill(root.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--artifact",
            "artifact-a",
            "--dry-run",
        ],
    );

    assert!(
        !output.status.success(),
        "--artifact without --compiled must fail"
    );
    assert_eq!(
        env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
}

#[test]
fn skill_activate_compiled_rejects_missing_artifact_without_mutation() {
    let root = TestDir::new("skill-activate-compiled-missing");
    let home = TestDir::new("skill-activate-compiled-missing-home");
    write_good_skill(root.path(), "demo");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--dry-run",
        ],
    );

    assert!(
        !output.status.success(),
        "compiled activation must fail closed"
    );
    assert_eq!(
        env["error"]["code"],
        Value::String("POLICY_BLOCKED".to_string())
    );
    assert_eq!(
        env["error"]["details"]["reason"],
        Value::String("compiled_artifact_missing".to_string())
    );
    assert!(
        env["error"]["details"]["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action.as_str().is_some_and(
                |text| text.contains("skill compile demo --agent codex --profile default")
            )),
        "missing artifact should recommend compile: {env}"
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "compiled dry-run precondition failure must not initialize registry state"
    );
    assert!(
        !home.path().join(".agents/skills/demo").exists(),
        "compiled precondition failure must not create a projection"
    );
}

#[test]
fn skill_activate_compiled_rejects_experimental_artifact_and_normal_activation_still_works() {
    let root = TestDir::new("skill-activate-compiled-experimental");
    let home = TestDir::new("skill-activate-compiled-experimental-home");
    write_compile_ready_skill(root.path(), "demo");

    let (compile_output, compile_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );
    assert!(
        compile_output.status.success(),
        "compile should write artifact: {compile_env}"
    );
    let artifact_id = compile_env["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_string();

    let (compiled_output, compiled_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--artifact",
            &artifact_id,
        ],
    );

    assert!(
        !compiled_output.status.success(),
        "experimental artifact must fail compiled activation"
    );
    assert_eq!(
        compiled_env["error"]["code"],
        Value::String("POLICY_BLOCKED".to_string())
    );
    assert_eq!(
        compiled_env["error"]["details"]["reason"],
        Value::String("compiled_artifact_not_valid".to_string())
    );
    assert_eq!(
        compiled_env["error"]["details"]["reports"][0]["artifact_id"],
        Value::String(artifact_id.clone())
    );
    assert_eq!(
        compiled_env["error"]["details"]["reports"][0]["status"],
        Value::String("experimental".to_string())
    );
    assert!(
        !home.path().join(".agents/skills/demo").exists(),
        "failed compiled activation must not create a projection"
    );

    let (normal_output, normal_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert!(
        normal_output.status.success(),
        "normal activation should still pass: {normal_env}"
    );
    assert!(
        home.path().join(".agents/skills/demo").exists(),
        "normal activation should create the source projection"
    );
}

#[test]
fn skill_activate_compiled_rejects_agent_profile_mismatch() {
    let root = TestDir::new("skill-activate-compiled-mismatch");
    let home = TestDir::new("skill-activate-compiled-mismatch-home");
    write_compile_ready_skill(root.path(), "demo");

    let (compile_output, compile_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "compile", "demo", "--agent", "claude"],
    );
    assert!(
        compile_output.status.success(),
        "compile should write artifact: {compile_env}"
    );
    let artifact_id = compile_env["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_string();

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--artifact",
            &artifact_id,
            "--dry-run",
        ],
    );

    assert!(!output.status.success(), "agent mismatch must fail");
    assert_eq!(
        env["error"]["code"],
        Value::String("POLICY_BLOCKED".to_string())
    );
    assert_eq!(
        env["error"]["details"]["reason"],
        Value::String("compiled_artifact_agent_profile_mismatch".to_string())
    );
    assert_eq!(
        env["error"]["details"]["reports"][0]["manifest"]["agent"],
        Value::String("claude".to_string())
    );
}

#[test]
fn skill_activate_compiled_wraps_malformed_manifest_as_policy_block() {
    let root = TestDir::new("skill-activate-compiled-malformed-manifest");
    let home = TestDir::new("skill-activate-compiled-malformed-manifest-home");
    write_compile_ready_skill(root.path(), "demo");
    let artifact_id = "broken-manifest";
    let artifact_root = root
        .path()
        .join("state/compiled/skills/demo")
        .join(artifact_id);
    fs::create_dir_all(&artifact_root).expect("artifact root");
    fs::write(artifact_root.join("manifest.json"), "{not-json").expect("malformed manifest");

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "codex",
            "--compiled",
            "--artifact",
            artifact_id,
            "--dry-run",
        ],
    );

    assert!(
        !output.status.success(),
        "malformed compiled artifact must fail closed"
    );
    assert_eq!(
        env["error"]["code"],
        Value::String("POLICY_BLOCKED".to_string())
    );
    assert_eq!(
        env["error"]["details"]["reason"],
        Value::String("compiled_artifact_not_valid".to_string())
    );
    assert_eq!(
        env["error"]["details"]["reports"][0]["artifact_id"],
        Value::String(artifact_id.to_string())
    );
    assert_eq!(
        env["error"]["details"]["reports"][0]["findings"][0]["id"],
        Value::String("manifest_schema_mismatch".to_string())
    );
    assert!(
        !home.path().join(".agents/skills/demo").exists(),
        "malformed compiled artifact must not create a projection"
    );
}

#[test]
fn skill_activate_lists_repairs_and_deactivates_user_symlink() {
    let root = TestDir::new("skill-activate-cycle");
    let home = TestDir::new("skill-activate-cycle-home");
    write_good_skill(root.path(), "demo");

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert!(
        activate_output.status.success(),
        "activate should succeed: {activate_env}"
    );
    assert_eq!(
        activate_env["cmd"],
        Value::String("skill.activate".to_string())
    );
    assert_eq!(activate_env["data"]["noop"], Value::Bool(false));
    assert_eq!(
        activate_env["data"]["target"]["path"],
        Value::String(home.path().join(".agents/skills").display().to_string())
    );
    assert_eq!(
        activate_env["data"]["binding"]["binding_id"],
        Value::String("bind_codex_default_user".to_string())
    );
    let projected = home.path().join(".agents/skills/demo");
    assert!(
        fs::symlink_metadata(&projected)
            .expect("projected symlink metadata")
            .file_type()
            .is_symlink(),
        "activation should create a symlink projection"
    );

    let (list_output, list_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "active", "list", "--agent", "codex"],
    );
    assert!(list_output.status.success(), "active list should succeed");
    assert_eq!(list_env["data"]["count"], Value::from(1));
    assert_eq!(
        list_env["data"]["items"][0]["skill"],
        Value::String("demo".to_string())
    );
    assert_eq!(
        list_env["data"]["items"][0]["status"],
        Value::String("healthy".to_string())
    );
    assert_eq!(
        list_env["data"]["items"][0]["visible_to_agent"],
        Value::String("not_checked".to_string())
    );

    let (noop_output, noop_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert!(
        noop_output.status.success(),
        "idempotent activate should pass"
    );
    assert_eq!(noop_env["data"]["noop"], Value::Bool(true));

    fs::remove_file(&projected).expect("remove symlink to simulate missing projection");
    let (repair_output, repair_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert!(
        repair_output.status.success(),
        "repair activate should pass"
    );
    assert_eq!(repair_env["data"]["noop"], Value::Bool(false));
    assert!(
        fs::symlink_metadata(&projected)
            .expect("repaired symlink metadata")
            .file_type()
            .is_symlink(),
        "repair should restore missing symlink"
    );

    let (deactivate_output, deactivate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "deactivate", "demo", "--agent", "codex"],
    );
    assert!(
        deactivate_output.status.success(),
        "deactivate should succeed: {deactivate_env}"
    );
    assert_eq!(
        deactivate_env["cmd"],
        Value::String("skill.deactivate".to_string())
    );
    assert!(
        fs::symlink_metadata(&projected).is_err(),
        "deactivate should remove safe symlink projection"
    );

    let (_, list_after_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "active", "list", "--agent", "codex"],
    );
    assert_eq!(list_after_env["data"]["count"], Value::from(0));
}

#[test]
fn skill_deactivate_fails_closed_for_copy_projection() {
    let root = TestDir::new("skill-deactivate-copy");
    let home = TestDir::new("skill-deactivate-copy-home");
    write_good_skill(root.path(), "demo");

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill", "activate", "demo", "--agent", "codex", "--method", "copy",
        ],
    );
    assert!(
        activate_output.status.success(),
        "copy activation should succeed: {activate_env}"
    );
    let projected = home.path().join(".agents/skills/demo");
    assert!(
        projected.join("SKILL.md").is_file(),
        "copy projection exists"
    );

    let (deactivate_output, deactivate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "deactivate", "demo", "--agent", "codex"],
    );
    assert!(
        !deactivate_output.status.success(),
        "copy deactivate should fail closed"
    );
    assert_eq!(
        deactivate_env["error"]["code"],
        Value::String("POLICY_BLOCKED".to_string())
    );
    assert!(
        projected.join("SKILL.md").is_file(),
        "failed closed deactivate must not delete copy projection"
    );
}

#[test]
fn skill_activate_rejects_observed_target_before_projection_write() {
    let root = TestDir::new("skill-activate-observed");
    let home = TestDir::new("skill-activate-observed-home");
    write_good_skill(root.path(), "demo");

    let observed_path = home.path().join("observed/skills");
    fs::create_dir_all(&observed_path).expect("create observed target");
    let observed_arg = observed_path.to_string_lossy().to_string();
    let (target_output, target_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "target",
            "add",
            "--agent",
            "codex",
            "--path",
            &observed_arg,
            "--ownership",
            "observed",
        ],
    );
    assert!(
        target_output.status.success(),
        "target add should pass: {target_env}"
    );
    let target_id = target_env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &[
            "skill", "activate", "demo", "--agent", "codex", "--target", target_id,
        ],
    );
    assert!(
        !activate_output.status.success(),
        "observed target activation must fail"
    );
    assert_eq!(
        activate_env["error"]["code"],
        Value::String("TARGET_NOT_MANAGED".to_string())
    );
    assert!(
        !observed_path.join("demo").exists(),
        "activation must fail before writing observed target"
    );
}

#[test]
fn skill_active_list_reports_missing_target_without_visibility_claim() {
    let root = TestDir::new("skill-active-target-missing");
    let home = TestDir::new("skill-active-target-missing-home");
    write_good_skill(root.path(), "demo");

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert!(
        activate_output.status.success(),
        "activate should pass: {activate_env}"
    );
    fs::remove_dir_all(home.path().join(".agents/skills")).expect("remove target dir");

    let (list_output, list_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "active", "list", "--agent", "codex"],
    );
    assert!(list_output.status.success(), "active list should succeed");
    assert_eq!(
        list_env["data"]["items"][0]["status"],
        Value::String("target_missing".to_string())
    );
    assert_eq!(
        list_env["data"]["visibility_claim"],
        Value::String("not_checked".to_string())
    );
}

#[test]
fn skill_active_list_reports_missing_source() {
    let root = TestDir::new("skill-active-source-missing");
    let home = TestDir::new("skill-active-source-missing-home");
    write_good_skill(root.path(), "demo");

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert!(
        activate_output.status.success(),
        "activate should pass: {activate_env}"
    );
    fs::remove_dir_all(root.path().join("skills/demo")).expect("remove source skill");

    let (list_output, list_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "active", "list", "--agent", "codex"],
    );
    assert!(list_output.status.success(), "active list should succeed");
    assert_eq!(
        list_env["data"]["items"][0]["status"],
        Value::String("source_missing".to_string())
    );
}
