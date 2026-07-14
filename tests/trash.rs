use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[path = "trash/activation_edge_cases.rs"]
mod activation_edge_cases;
mod common;

use common::actions::save_skill;
use common::{TestDir, operations_log, run_loom, run_loom_with_env, write_file, write_skill};

fn run_with_home(root: &Path, home: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let home = home.to_string_lossy().to_string();
    run_loom_with_env(root, &[("HOME", &home)], args)
}

fn write_activatable_skill(root: &Path, skill: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when testing trash activation cleanup.\n---\n# {skill}\n"
        ),
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read json file")).expect("parse json file")
}

fn write_json(path: &Path, value: &Value) {
    let mut raw = serde_json::to_string_pretty(value).expect("serialize json");
    raw.push('\n');
    write_file(path, &raw);
}

fn read_optional(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

fn create_dir_symlink(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("create directory symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).expect("create directory symlink");
}

fn relative_path(from_dir: &Path, to: &Path) -> PathBuf {
    let from = fs::canonicalize(from_dir).expect("canonicalize relative path base");
    let to = fs::canonicalize(to).expect("canonicalize relative path target");
    let from_components = from.components().collect::<Vec<_>>();
    let to_components = to.components().collect::<Vec<_>>();
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for component in &from_components[common..] {
        if matches!(component, Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &to_components[common..] {
        relative.push(component.as_os_str());
    }
    relative
}

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn git_success(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: stderr={} stdout={}",
        args,
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn rollback_error_steps(env: &Value) -> Vec<String> {
    env["error"]["details"]["rollback_errors"]
        .as_array()
        .expect("rollback errors array")
        .iter()
        .filter_map(|error| error["step"].as_str().map(ToString::to_string))
        .collect()
}

#[test]
fn skill_trash_list_is_read_only_without_repo() {
    let root = TestDir::new("skill-trash-list-read-only");

    let (output, env) = run_loom(root.path(), &["skill", "trash", "list"]);

    assert_success(&output, "trash list");
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["items"], Value::Array(Vec::new()));
    assert!(!root.path().join(".git").exists());
    assert!(!root.path().join("state/events").exists());
    assert!(!root.path().join("state/registry").exists());
}

#[test]
fn skill_trash_add_lists_and_restores_latest_entry() {
    let root = TestDir::new("skill-trash-restore");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");

    let (trash_output, trash_env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");
    assert_eq!(trash_env["ok"], Value::Bool(true));
    let trash_id = trash_env["data"]["trash_id"]
        .as_str()
        .expect("trash id")
        .to_string();
    assert!(!root.path().join("skills/demo").exists());
    assert!(
        root.path()
            .join("trash")
            .join(&trash_id)
            .join("skill/SKILL.md")
            .exists()
    );
    assert!(
        root.path()
            .join("trash")
            .join(&trash_id)
            .join("metadata.json")
            .exists()
    );

    let (list_output, list_env) = run_loom(root.path(), &["skill", "trash", "list"]);
    assert_success(&list_output, "trash list");
    assert_eq!(list_env["ok"], Value::Bool(true));
    assert_eq!(
        list_env["data"]["items"][0]["trash_id"],
        Value::String(trash_id.clone())
    );
    assert_eq!(
        list_env["data"]["items"][0]["skill"],
        Value::String("demo".to_string())
    );
    assert!(
        !list_env["meta"]
            .as_object()
            .is_some_and(|meta| meta.contains_key("op_id")),
        "read-only trash list must not report an op_id"
    );

    let (restore_output, restore_env) =
        run_loom(root.path(), &["skill", "trash", "restore", "demo"]);
    assert_success(&restore_output, "trash restore");
    assert_eq!(restore_env["ok"], Value::Bool(true));
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(!root.path().join("trash").join(&trash_id).exists());
    let restored_paths = git_success(root.path(), &["show", "--name-only", "--pretty=", "HEAD"]);
    assert!(
        restored_paths.contains("trash/"),
        "restore commit omitted trash deletion: {restored_paths}"
    );
    assert_eq!(
        git_success(
            root.path(),
            &["status", "--porcelain", "--", &format!("trash/{trash_id}")]
        ),
        ""
    );
    let operations = operations_log(root.path());
    assert!(operations.contains(r#""intent":"skill.trash.add""#));
    assert!(operations.contains(r#""intent":"skill.trash.restore""#));
}

#[test]
fn skill_trash_add_removes_active_state_and_managed_symlink() {
    let root = TestDir::new("skill-trash-active-cleanup");
    let home = TestDir::new("skill-trash-active-cleanup-home");
    write_activatable_skill(root.path(), "demo");

    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let (claude_output, claude_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "claude"],
    );
    assert_success(&claude_output, &format!("claude activation: {claude_env}"));
    let live_paths = [
        home.path().join(".agents/skills/demo"),
        home.path().join(".claude/skills/demo"),
    ];
    assert!(live_paths.iter().all(|path| path.is_symlink()));

    let (trash_output, trash_env) =
        run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, &format!("trash add: {trash_env}"));

    let rules = read_json(&root.path().join("state/registry/rules.json"));
    let projections = read_json(&root.path().join("state/registry/projections.json"));
    assert_eq!(rules["rules"], Value::Array(Vec::new()));
    assert_eq!(projections["projections"], Value::Array(Vec::new()));
    assert!(
        live_paths
            .iter()
            .all(|path| !path.exists() && !path.is_symlink())
    );

    let (doctor_output, doctor_env) =
        run_with_home(root.path(), home.path(), &["workspace", "doctor"]);
    assert_success(&doctor_output, &format!("workspace doctor: {doctor_env}"));
    assert_eq!(doctor_env["data"]["healthy"], Value::Bool(true));

    let (restore_output, restore_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "trash", "restore", "demo"],
    );
    assert_success(&restore_output, &format!("trash restore: {restore_env}"));
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(
        live_paths
            .iter()
            .all(|path| !path.exists() && !path.is_symlink())
    );
    let rules = read_json(&root.path().join("state/registry/rules.json"));
    let projections = read_json(&root.path().join("state/registry/projections.json"));
    assert_eq!(rules["rules"], Value::Array(Vec::new()));
    assert_eq!(projections["projections"], Value::Array(Vec::new()));
}

#[test]
fn skill_trash_add_preserves_unrelated_staged_changes() {
    let root = TestDir::new("skill-trash-preserve-staged");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");

    write_file(&root.path().join("README.md"), "staged but unrelated\n");
    git_success(root.path(), &["add", "README.md"]);

    let (trash_output, _) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");

    let committed_paths = git_success(root.path(), &["show", "--name-only", "--pretty=", "HEAD"]);
    let committed_status =
        git_success(root.path(), &["show", "--name-status", "--pretty=", "HEAD"]);
    assert!(
        committed_status.contains("skills/demo/SKILL.md"),
        "trash commit omitted source deletion: {committed_status}"
    );
    assert!(committed_paths.contains("trash/"));
    assert!(
        !committed_paths.contains("README.md"),
        "trash commit included unrelated staged path: {committed_paths}"
    );
    assert_eq!(
        git_success(root.path(), &["status", "--porcelain", "--", "skills/demo"]),
        ""
    );
    assert_eq!(
        git_success(root.path(), &["status", "--porcelain", "--", "README.md"]),
        "A  README.md"
    );
}

#[test]
fn skill_trash_add_accepts_untracked_skill_and_preserves_unrelated_staged_changes() {
    let root = TestDir::new("skill-trash-untracked-source");
    let (init_output, _) = run_loom(root.path(), &["workspace", "init"]);
    assert_success(&init_output, "workspace init");
    write_skill(root.path(), "manual", "# Manual\n\nnever committed\n");

    write_file(&root.path().join("README.md"), "staged but unrelated\n");
    git_success(root.path(), &["add", "README.md"]);

    let (trash_output, trash_env) = run_loom(root.path(), &["skill", "trash", "add", "manual"]);
    assert_success(&trash_output, "trash add");
    assert_eq!(trash_env["ok"], Value::Bool(true));
    let trash_id = match trash_env["data"]["trash_id"].as_str() {
        Some(trash_id) => trash_id,
        None => panic!("trash add did not return a trash id: {trash_env}"),
    };
    assert!(!root.path().join("skills/manual").exists());
    assert!(
        root.path()
            .join("trash")
            .join(trash_id)
            .join("skill/SKILL.md")
            .exists()
    );

    let committed_paths = git_success(root.path(), &["show", "--name-only", "--pretty=", "HEAD"]);
    assert!(committed_paths.contains("trash/"));
    assert!(
        !committed_paths.contains("README.md"),
        "trash commit included unrelated staged path: {committed_paths}"
    );
    assert_eq!(
        git_success(root.path(), &["status", "--porcelain", "--", "README.md"]),
        "A  README.md"
    );
}

#[test]
fn skill_trash_add_dry_run_reports_plan_without_mutation() {
    let root = TestDir::new("skill-trash-add-dry-run");
    let home = TestDir::new("skill-trash-add-dry-run-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let live_path = home.path().join(".agents/skills/demo");
    let head_before = git_success(root.path(), &["rev-parse", "HEAD"]);
    let operations_before = operations_log(root.path());
    let rules_before = fs::read(root.path().join("state/registry/rules.json")).expect("read rules");
    let projections_before =
        fs::read(root.path().join("state/registry/projections.json")).expect("read projections");
    let command_events_path = root.path().join("state/events/commands.jsonl");
    let command_events_before = read_optional(&command_events_path);

    let (output, env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "trash", "add", "demo", "--dry-run"],
    );

    assert_success(&output, "trash add dry-run");
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(env["data"]["would_move"], Value::Bool(true));
    assert_eq!(
        env["data"]["activation_impact"]["removed_rule_count"],
        Value::from(1)
    );
    assert_eq!(
        env["data"]["activation_impact"]["removed_projection_ids"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        env["data"]["activation_impact"]["links"][0]["action"],
        Value::String("delete".to_string())
    );
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(!root.path().join("trash").exists());
    assert!(live_path.is_symlink());
    assert_eq!(
        fs::read(root.path().join("state/registry/rules.json")).expect("read rules after"),
        rules_before
    );
    assert_eq!(
        fs::read(root.path().join("state/registry/projections.json"))
            .expect("read projections after"),
        projections_before
    );
    assert_eq!(
        git_success(root.path(), &["rev-parse", "HEAD"]),
        head_before
    );
    assert_eq!(operations_log(root.path()), operations_before);
    assert_eq!(read_optional(&command_events_path), command_events_before);
    assert!(env["meta"]["op_id"].is_null());
}

#[test]
fn skill_trash_add_retains_unowned_live_path_but_removes_active_records() {
    let root = TestDir::new("skill-trash-retain-unowned");
    let home = TestDir::new("skill-trash-retain-unowned-home");
    write_activatable_skill(root.path(), "demo");
    write_activatable_skill(root.path(), "other");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let (other_output, other_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "other", "--agent", "codex"],
    );
    assert_success(&other_output, &format!("other activation: {other_env}"));
    let live_path = home.path().join(".agents/skills/demo");
    fs::remove_file(&live_path).expect("remove managed symlink");
    write_file(&live_path, "user-owned replacement\n");
    let bindings_before =
        fs::read(root.path().join("state/registry/bindings.json")).expect("read bindings");
    let targets_before =
        fs::read(root.path().join("state/registry/targets.json")).expect("read targets");

    let (trash_output, trash_env) =
        run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, &format!("trash add: {trash_env}"));
    assert_eq!(
        fs::read_to_string(&live_path).expect("read retained ordinary file"),
        "user-owned replacement\n"
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["action"],
        Value::String("retain".to_string())
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["reason"],
        Value::String("not_symlink".to_string())
    );
    let rules = read_json(&root.path().join("state/registry/rules.json"));
    let projections = read_json(&root.path().join("state/registry/projections.json"));
    assert_eq!(rules["rules"].as_array().map(Vec::len), Some(1));
    assert_eq!(rules["rules"][0]["skill_id"], Value::String("other".into()));
    assert_eq!(projections["projections"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        projections["projections"][0]["skill_id"],
        Value::String("other".into())
    );
    assert!(home.path().join(".agents/skills/other").is_symlink());
    assert_eq!(
        fs::read(root.path().join("state/registry/bindings.json")).expect("read bindings after"),
        bindings_before
    );
    assert_eq!(
        fs::read(root.path().join("state/registry/targets.json")).expect("read targets after"),
        targets_before
    );
}

#[test]
fn skill_trash_restore_refuses_to_overwrite_existing_skill() {
    let root = TestDir::new("skill-trash-restore-conflict");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");
    let (trash_output, _) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");

    write_skill(root.path(), "demo", "# Demo\n\nreplacement\n");
    let (restore_output, restore_env) =
        run_loom(root.path(), &["skill", "trash", "restore", "demo"]);

    assert!(
        !restore_output.status.success(),
        "restore unexpectedly succeeded"
    );
    assert_eq!(restore_env["ok"], Value::Bool(false));
    assert_eq!(
        restore_env["error"]["code"],
        Value::String("ARG_INVALID".to_string())
    );
    let live = fs::read_to_string(root.path().join("skills/demo/SKILL.md")).expect("read skill");
    assert!(live.contains("replacement"));
}

#[test]
fn skill_trash_restore_missing_entry_reports_specific_error_code() {
    let root = TestDir::new("skill-trash-restore-missing");

    let (output, env) = run_loom(
        root.path(),
        &[
            "skill",
            "trash",
            "restore",
            "demo",
            "--trash-id",
            "missing-trash",
        ],
    );

    assert!(!output.status.success(), "restore unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("TRASH_ENTRY_NOT_FOUND".to_string())
    );
}

#[test]
fn skill_trash_purge_removes_one_trash_entry() {
    let root = TestDir::new("skill-trash-purge");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");

    let (trash_output, trash_env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");
    let trash_id = trash_env["data"]["trash_id"].as_str().expect("trash id");

    let (purge_output, purge_env) = run_loom(root.path(), &["skill", "trash", "purge", trash_id]);
    assert_success(&purge_output, "trash purge");
    assert_eq!(purge_env["ok"], Value::Bool(true));
    assert!(!root.path().join("trash").join(trash_id).exists());
    let purged_paths = git_success(root.path(), &["show", "--name-only", "--pretty=", "HEAD"]);
    assert!(
        purged_paths.contains("trash/"),
        "purge commit omitted trash deletion: {purged_paths}"
    );
    assert_eq!(
        git_success(
            root.path(),
            &["status", "--porcelain", "--", &format!("trash/{trash_id}")]
        ),
        ""
    );
    assert!(operations_log(root.path()).contains(r#""intent":"skill.trash.purge""#));
}

#[test]
fn skill_trash_purge_dry_run_reports_plan_without_mutation() {
    let root = TestDir::new("skill-trash-purge-dry-run");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");
    let (trash_output, trash_env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&trash_output, "trash add");
    let trash_id = trash_env["data"]["trash_id"].as_str().expect("trash id");
    let head_before = git_success(root.path(), &["rev-parse", "HEAD"]);
    let operations_before = operations_log(root.path());

    let (purge_output, purge_env) = run_loom(
        root.path(),
        &["skill", "trash", "purge", trash_id, "--dry-run"],
    );

    assert_success(&purge_output, "trash purge dry-run");
    assert_eq!(purge_env["ok"], Value::Bool(true));
    assert_eq!(purge_env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(purge_env["data"]["would_purge"], Value::Bool(true));
    assert!(root.path().join("trash").join(trash_id).exists());
    assert_eq!(
        git_success(root.path(), &["rev-parse", "HEAD"]),
        head_before
    );
    assert_eq!(operations_log(root.path()), operations_before);
    assert!(purge_env["meta"]["op_id"].is_null());
}

#[test]
fn skill_trash_purge_missing_entry_reports_specific_error_code() {
    let root = TestDir::new("skill-trash-purge-missing");

    let (output, env) = run_loom(root.path(), &["skill", "trash", "purge", "missing-trash"]);

    assert!(!output.status.success(), "purge unexpectedly succeeded");
    assert_eq!(env["ok"], Value::Bool(false));
    assert_eq!(
        env["error"]["code"],
        Value::String("TRASH_ENTRY_NOT_FOUND".to_string())
    );
}

#[test]
fn skill_trash_add_reports_audit_restore_rollback_errors() {
    let root = TestDir::new("skill-trash-rollback-errors");
    let home = TestDir::new("skill-trash-rollback-errors-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
            ("LOOM_ROLLBACK_FAULT_INJECT", "restore_registry_audit_state"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(
        !output.status.success(),
        "faulted trash add unexpectedly succeeded"
    );
    assert!(
        rollback_error_steps(&env).contains(&"restore_registry_audit_state".to_string()),
        "missing rollback error details: {env}"
    );
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(home.path().join(".agents/skills/demo").is_symlink());
    assert_eq!(
        read_json(&root.path().join("state/registry/rules.json"))["rules"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        read_json(&root.path().join("state/registry/projections.json"))["projections"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn skill_trash_add_reports_activation_state_rollback_errors() {
    let root = TestDir::new("skill-trash-state-rollback-errors");
    let home = TestDir::new("skill-trash-state-rollback-errors-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
            ("LOOM_ROLLBACK_FAULT_INJECT", "restore_registry_state"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(!output.status.success(), "faulted trash add must fail");
    assert!(
        rollback_error_steps(&env).contains(&"restore_registry_state".to_string()),
        "missing registry rollback error details: {env}"
    );
    assert!(root.path().join("skills/demo/SKILL.md").exists());
    assert!(home.path().join(".agents/skills/demo").is_symlink());
}
