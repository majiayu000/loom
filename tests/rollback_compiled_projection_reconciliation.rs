use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

mod common;

use common::actions::save_skill;
use common::{TestDir, run_loom_with_env, write_file, write_skill};

struct CompiledRollbackFixture {
    root: TestDir,
    home: TestDir,
    artifact_id: String,
    projection_path: PathBuf,
    v1_ref: String,
}

fn compiled_rollback_fixture(prefix: &str) -> CompiledRollbackFixture {
    let root = TestDir::new(prefix);
    let home = TestDir::new(&format!("{prefix}-home"));
    write_compile_ready_skill(root.path(), "demo", "source v1");
    write_passing_eval(root.path(), "demo");
    assert!(save_skill(root.path(), "demo").0.status.success());
    let v1_ref = git_output(root.path(), &["rev-parse", "HEAD"]);

    let (_compile_output, compile_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "compile", "demo", "--agent", "codex"],
    );
    assert_eq!(compile_env["ok"], json!(true), "{compile_env}");
    let artifact_id = compile_env["data"]["artifact"]["artifact_id"]
        .as_str()
        .expect("artifact id")
        .to_string();

    let (activate_output, activate_env) = run_with_home(
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
        activate_output.status.success(),
        "compiled activation failed: {activate_env}"
    );
    let projection_path = home.path().join(".agents/skills/demo");
    assert!(
        projection_path
            .join(".loom/compiled/projection.json")
            .is_file()
    );

    CompiledRollbackFixture {
        root,
        home,
        artifact_id,
        projection_path,
        v1_ref,
    }
}

#[test]
fn rollback_compiled_activation_uses_compiled_recovery_from_registry_operation() {
    let fixture = compiled_rollback_fixture("rollback-compiled-operation-recovery");
    fs::remove_dir_all(&fixture.projection_path).expect("remove live compiled projection");

    let rollback_env = rollback_after_source_update(&fixture);

    let item = &rollback_env["data"]["projection_reconciliation"]["items"][0];
    assert_eq!(item["method"], json!("materialize"));
    assert_eq!(item["status"], json!("missing_projection_path"));
    assert_eq!(
        item["compiled_activation"]["metadata_source"],
        json!("registry_operation")
    );
    assert_eq!(
        item["compiled_activation"]["artifact_id"],
        json!(fixture.artifact_id)
    );

    let action = &item["next_action"];
    assert_eq!(action["type"], json!("manual_review_required"));
    assert!(
        action["command"].is_null(),
        "must not emit raw project command"
    );
    let commands = command_strings(action);
    assert_eq!(commands.len(), 2, "{action}");
    assert!(
        commands[0].contains(" skill compile verify demo --artifact "),
        "{commands:?}"
    );
    assert!(
        commands[1].contains(" skill activate demo --agent codex --compiled --artifact "),
        "{commands:?}"
    );
    assert!(
        commands[1].contains(" --scope user ") && commands[1].contains(" --target "),
        "{commands:?}"
    );
    assert_no_raw_project_command(&commands);
    assert_eq!(
        rollback_env["data"]["projection_reconciliation"]["next_actions"][0],
        *action
    );
}

#[test]
fn rollback_compiled_activation_uses_live_metadata_when_operation_payload_is_legacy() {
    let fixture = compiled_rollback_fixture("rollback-compiled-live-metadata-recovery");
    scrub_compiled_payload_from_activation_operation(fixture.root.path());
    git_output(
        fixture.root.path(),
        &["add", "--", "state/registry/ops/operations.jsonl"],
    );
    git_output(
        fixture.root.path(),
        &[
            "commit",
            "-m",
            "test: scrub compiled operation payload",
            "--",
            "state/registry/ops/operations.jsonl",
        ],
    );

    let rollback_env = rollback_after_source_update(&fixture);

    let item = &rollback_env["data"]["projection_reconciliation"]["items"][0];
    assert_eq!(item["method"], json!("materialize"));
    assert_eq!(item["status"], json!("requires_reapply"));
    assert_eq!(
        item["compiled_activation"]["metadata_source"],
        json!("live_projection_metadata")
    );
    assert_eq!(
        item["compiled_activation"]["artifact_id"],
        json!(fixture.artifact_id)
    );
    let action = &item["next_action"];
    assert_eq!(action["type"], json!("manual_review_required"));
    assert!(
        action["command"].is_null(),
        "must not emit raw project command"
    );
    let commands = command_strings(action);
    assert_eq!(commands.len(), 1, "{action}");
    assert!(
        commands[0].contains(" skill compile verify demo --artifact "),
        "{commands:?}"
    );
    assert_no_raw_project_command(&commands);
}

fn rollback_after_source_update(fixture: &CompiledRollbackFixture) -> Value {
    write_compile_ready_skill(fixture.root.path(), "demo", "source v2");
    assert!(save_skill(fixture.root.path(), "demo").0.status.success());
    let (rollback_output, rollback_env) = run_with_home(
        fixture.root.path(),
        fixture.home.path(),
        &["skill", "rollback", "demo", "--to", &fixture.v1_ref],
    );
    assert!(
        rollback_output.status.success(),
        "rollback failed: stderr={} stdout={}",
        String::from_utf8_lossy(&rollback_output.stderr),
        String::from_utf8_lossy(&rollback_output.stdout)
    );
    assert_eq!(rollback_env["ok"], json!(true), "{rollback_env}");
    rollback_env
}

fn run_with_home(root: &Path, home: &Path, args: &[&str]) -> (std::process::Output, Value) {
    let home = home.to_string_lossy().to_string();
    run_loom_with_env(root, &[("HOME", &home)], args)
}

fn write_compile_ready_skill(root: &Path, skill: &str, marker: &str) {
    write_skill(
        root,
        skill,
        &format!(
            "---\nname: {skill}\ndescription: Use when testing compiled rollback reconciliation.\n---\n# {skill}\n\n{marker}\n\nUse when testing compiled rollback reconciliation.\n\nDo not use for production claims.\n"
        ),
    );
}

fn write_passing_eval(root: &Path, skill: &str) {
    write_file(
        &root.join("skills").join(skill).join("evals/tasks.jsonl"),
        r#"{"id":"happy-path","task":"Run the compiled rollback eval","output":"Done with concise result","trace":["read SKILL.md","checked output"],"metrics":{"tokens":40,"commands":1},"checks":{"outcome_contains":["Done"],"process_contains":["SKILL.md"],"style_contains":["concise"],"max_tokens":100,"max_commands":3}}
"#,
    );
}

fn scrub_compiled_payload_from_activation_operation(root: &Path) {
    let path = root.join("state/registry/ops/operations.jsonl");
    let raw = fs::read_to_string(&path).expect("read operations log");
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut value: Value = serde_json::from_str(line).expect("parse operation record");
        if value["intent"] == json!("skill.activate")
            && let Some(payload) = value["payload"].as_object_mut()
        {
            payload.remove("compiled");
        }
        lines.push(serde_json::to_string(&value).expect("serialize operation record"));
    }
    fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write operations log");
}

fn command_strings(action: &Value) -> Vec<&str> {
    action["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|command| command.as_str().expect("command string"))
        .collect()
}

fn assert_no_raw_project_command(commands: &[&str]) {
    assert!(
        commands
            .iter()
            .all(|command| !command.contains(" skill project ")),
        "compiled rollback recovery must not emit raw project command: {commands:?}"
    );
}

fn git_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("user.name=Loom Test")
        .arg("-c")
        .arg("user.email=loom@example.invalid")
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
