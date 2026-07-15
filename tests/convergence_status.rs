mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use common::{
    TestDir, run_loom, run_loom_with_env, write_file, write_minimal_registry_state, write_skill,
};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "loom@example.com"]);
    git(root, &["config", "user.name", "Loom Test"]);
}

fn commit_all(root: &Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "fixture"]);
}

fn write_json(path: &Path, value: &Value) {
    write_file(
        path,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(value).expect("serialize json")
        ),
    );
}

fn make_projection_converged(root: &Path, target_root: &Path, skill: &str, agent: &str) {
    let projection = target_root.join(skill);
    fs::create_dir_all(target_root).expect("create target");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("skills").join(skill), &projection)
        .expect("create projection symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(root.join("skills").join(skill), &projection)
        .expect("create projection symlink");

    write_minimal_registry_state(root, 1);
    let registry = root.join("state/registry");
    let mut targets: Value =
        serde_json::from_str(&fs::read_to_string(registry.join("targets.json")).expect("targets"))
            .expect("parse targets");
    targets["targets"][0]["agent"] = json!(agent);
    targets["targets"][0]["path"] = json!(target_root);
    write_json(&registry.join("targets.json"), &targets);

    let mut bindings: Value = serde_json::from_str(
        &fs::read_to_string(registry.join("bindings.json")).expect("bindings"),
    )
    .expect("parse bindings");
    bindings["bindings"][0]["agent"] = json!(agent);
    write_json(&registry.join("bindings.json"), &bindings);

    let mut rules: Value =
        serde_json::from_str(&fs::read_to_string(registry.join("rules.json")).expect("rules"))
            .expect("parse rules");
    rules["rules"][0]["skill_id"] = json!(skill);
    write_json(&registry.join("rules.json"), &rules);

    let mut projections: Value = serde_json::from_str(
        &fs::read_to_string(registry.join("projections.json")).expect("projections"),
    )
    .expect("parse projections");
    projections["projections"][0]["skill_id"] = json!(skill);
    projections["projections"][0]["materialized_path"] = json!(projection);
    write_json(&registry.join("projections.json"), &projections);
}

#[test]
fn empty_and_unsupported_axes_are_explicit_and_legacy_sync_state_matches() {
    let root = TestDir::new("convergence-empty");
    let (output, first) = run_loom(root.path(), &["workspace", "status"]);
    assert!(output.status.success(), "workspace status failed: {first}");
    let convergence = &first["data"]["convergence"];
    assert_eq!(
        convergence["registry_transport"]["state"],
        json!("LOCAL_ONLY")
    );
    assert_eq!(first["meta"]["sync_state"], json!("LOCAL_ONLY"));
    assert_eq!(convergence["projections"]["state"], json!("not_applicable"));
    assert_eq!(convergence["projections"]["items"], json!([]));
    assert_eq!(convergence["visibility"]["state"], json!("unsupported"));
    assert!(convergence["observed_at"].as_str().is_some());
    assert_eq!(convergence["complete"], json!(true));

    let (_, second) = run_loom(root.path(), &["workspace", "status"]);
    for axis in ["registry_transport", "projections", "visibility"] {
        assert_eq!(
            first["data"]["convergence"][axis]["state"],
            second["data"]["convergence"][axis]["state"]
        );
    }
    assert!(!root.path().join("state/registry").exists());
}

#[test]
fn remote_synced_projection_stale_remains_a_cross_axis_state() {
    let root = TestDir::new("convergence-cross-axis");
    let bare = TestDir::new("convergence-cross-axis-remote");
    init_git(root.path());
    write_skill(
        root.path(),
        "model-onboarding",
        "---\nname: model-onboarding\ndescription: Test status.\n---\n# Test\n",
    );
    write_minimal_registry_state(root.path(), 1);
    write_file(&root.path().join("state/registry/ops/operations.jsonl"), "");
    commit_all(root.path());
    git(bare.path(), &["init", "--bare"]);
    git(
        root.path(),
        &["remote", "add", "origin", &bare.path().to_string_lossy()],
    );
    git(root.path(), &["push", "-u", "origin", "main"]);

    let (output, env) = run_loom(root.path(), &["workspace", "status"]);
    assert!(output.status.success(), "workspace status failed: {env}");
    let convergence = &env["data"]["convergence"];
    assert_eq!(convergence["registry_transport"]["state"], json!("SYNCED"));
    assert_eq!(env["meta"]["sync_state"], json!("SYNCED"));
    assert_eq!(convergence["projections"]["state"], json!("missing"));
    assert_eq!(convergence["complete"], json!(true));
    assert!(convergence.get("overall").is_none());
}

#[test]
fn projection_converged_without_agent_evidence_is_unknown_not_visible() {
    let root = TestDir::new("convergence-projection-only");
    let target = TestDir::new("convergence-projection-only-target");
    init_git(root.path());
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Test convergence.\n---\n# Demo\n",
    );
    make_projection_converged(root.path(), target.path(), "demo", "claude");
    commit_all(root.path());

    let (output, env) = run_loom(root.path(), &["skill", "inspect", "demo"]);
    assert!(output.status.success(), "skill inspect failed: {env}");
    let convergence = &env["data"]["convergence"];
    assert_eq!(
        convergence["registry_transport"]["state"],
        json!("LOCAL_ONLY")
    );
    assert_eq!(convergence["projections"]["state"], json!("converged"));
    assert_eq!(convergence["visibility"]["state"], json!("unknown"));
    assert_eq!(convergence["complete"], json!(false));
    assert_eq!(convergence["incomplete_axes"], json!(["visibility"]));
}

#[test]
fn visibility_read_failure_keeps_independent_axes_and_names_partial_collection() {
    let root = TestDir::new("convergence-axis-failure");
    let home = TestDir::new("convergence-axis-failure-home");
    init_git(root.path());
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Test convergence.\n---\n# Demo\n",
    );
    commit_all(root.path());
    write_file(
        &home.path().join(".codex/config.toml"),
        "[[skills.config]]\nname =\n",
    );
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_value)],
        &["skill", "inspect", "demo", "--agent", "codex"],
    );
    assert!(output.status.success(), "skill inspect failed: {env}");
    let convergence = &env["data"]["convergence"];
    assert_eq!(
        convergence["registry_transport"]["state"],
        json!("LOCAL_ONLY")
    );
    assert_eq!(convergence["projections"]["state"], json!("not_applicable"));
    assert_eq!(convergence["visibility"]["state"], json!("error"));
    assert!(convergence["visibility"]["errors"][0]["code"].is_string());
    assert_eq!(convergence["complete"], json!(false));
    assert_eq!(convergence["incomplete_axes"], json!(["visibility"]));
}

#[test]
fn sync_status_adds_registry_transport_and_preserves_remote_mirror() {
    let root = TestDir::new("convergence-sync-status");
    let (output, env) = run_loom(root.path(), &["sync", "status"]);
    assert!(output.status.success(), "sync status failed: {env}");
    assert_eq!(
        env["data"]["registry_transport"]["state"],
        json!("LOCAL_ONLY")
    );
    assert_eq!(
        env["data"]["registry_transport"]["state"],
        env["data"]["remote"]["sync_state"]
    );
    assert_eq!(env["meta"]["sync_state"], json!("LOCAL_ONLY"));
}
