use std::{fs, path::Path, process::Command};

use serde_json::{Value, json};

use super::skill_convergence_executor::apply_plan;
use super::*;

#[test]
fn remote_failure_preserves_local_completion() {
    let fixture = projected_fixture();
    let remote = common::TestDir::new("convergence-remote-retry");
    let convergence_operations_before = convergence_operation_count(fixture.root.path());
    change_source(&fixture, "remote pending bytes\n");
    let (output, plan) = plan_converge(&fixture, &["--push-remote"]);
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, applied) = apply_plan(&fixture, &plan, "remote-pending", &[]);
    assert!(output.status.success(), "partial apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(data["local_state"], json!("complete"));
    assert_eq!(data["complete"], json!(false));
    assert_eq!(data["outcome"], json!("local_complete_remote_pending"));
    assert_eq!(
        data["completion_blockers"],
        json!(["registry.remote_pending"])
    );
    assert_eq!(
        data["convergence"]["registry_transport"]["state"],
        json!("PENDING_PUSH")
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/details.txt"))
            .expect("read local projection"),
        "remote pending bytes\n"
    );
    assert!(data["source"]["commit"].is_string());
    assert!(
        data["next_actions"][0]["cmd"]
            .as_str()
            .is_some_and(|command| command.contains("$IDEMPOTENCY_KEY"))
    );
    let aggregate_effects = data["evidence"].clone();
    assert_eq!(data["evidence"], aggregate_effects);
    assert_eq!(data["evidence"]["remote"]["state"], json!("pending_push"));
    assert_eq!(
        convergence_operation_count(fixture.root.path()),
        convergence_operations_before,
        "convergence must not append a registry ops ledger row"
    );

    let (wrong_key_output, wrong_key) = apply_plan(&fixture, &plan, "different-key", &[]);
    assert!(
        !wrong_key_output.status.success(),
        "different key must not own pending retry: {wrong_key}"
    );
    assert_eq!(wrong_key["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        wrong_key["error"]["details"]["conflict"]["code"],
        json!("IDEMPOTENCY_KEY_REUSED")
    );

    let operations_path = fixture
        .root
        .path()
        .join("state/registry/ops/operations.jsonl");
    let checkpoint_path = fixture
        .root
        .path()
        .join("state/registry/ops/checkpoint.json");
    let operations_before_retry = fs::read(&operations_path).expect("read operations before retry");
    let checkpoint_before_retry = fs::read(&checkpoint_path).expect("read checkpoint before retry");
    git(remote.path(), &["init", "--bare"]);
    let remote_path = remote.path().to_str().expect("remote path");
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote_path],
    );
    git(
        fixture.root.path(),
        &[
            "commit",
            "--allow-empty",
            "-m",
            "test: unrelated later commit",
        ],
    );
    let later_local_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let convergence_id = data["convergence_id"].clone();
    let source_commit = data["source"]["commit"].clone();
    let (output, retried) = apply_plan(&fixture, &plan, "remote-pending", &[]);
    assert!(output.status.success(), "remote retry failed: {retried}");
    let retried_data = &retried["data"];
    assert_eq!(retried_data["convergence_id"], convergence_id);
    assert_eq!(retried_data["source"]["commit"], source_commit);
    assert_eq!(retried_data["complete"], json!(true));
    assert_eq!(retried_data["outcome"], json!("complete"));
    assert_eq!(retried_data["completion_blockers"], json!([]));
    let transport = &retried_data["convergence"]["registry_transport"];
    assert_eq!(transport["state"], json!("SYNCED"));
    assert_eq!(
        transport["evidence"]["scope"],
        json!("exact_convergence_commit")
    );
    assert_eq!(
        transport["evidence"]["global_state_before_scope_override"],
        json!("PENDING_PUSH")
    );
    assert_eq!(retried_data["evidence"], aggregate_effects);
    assert_eq!(
        retried_data["evidence"]["remote"]["state"],
        json!("pending_push"),
        "retry must not rewrite immutable aggregate evidence"
    );
    assert_eq!(
        convergence_operation_count(fixture.root.path()),
        convergence_operations_before,
        "remote retry must not append a registry ops ledger row"
    );
    assert_eq!(
        fs::read(&operations_path).expect("read operations after retry"),
        operations_before_retry,
        "convergence transport must not acknowledge or rewrite the shared operations ledger"
    );
    assert_eq!(
        fs::read(&checkpoint_path).expect("read checkpoint after retry"),
        checkpoint_before_retry,
        "convergence transport must not advance the shared operations checkpoint"
    );
    let expected_boundary = retried_data["applied"]["registry_commit"]
        .as_str()
        .or_else(|| retried_data["applied"]["source_commit"].as_str())
        .expect("recorded convergence boundary");
    assert_eq!(
        transport["evidence"]["pushed_commit"],
        json!(expected_boundary)
    );
    let local_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        local_head.trim(),
        later_local_head.trim(),
        "convergence transport must not rewrite a later local HEAD"
    );
    let remote_head = Command::new("git")
        .arg("--git-dir")
        .arg(remote.path())
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .expect("inspect remote main after retry");
    assert!(
        remote_head.status.success(),
        "remote main must exist after retry"
    );
    assert_eq!(
        String::from_utf8(remote_head.stdout)
            .expect("remote head utf8")
            .trim(),
        expected_boundary,
        "convergence transport must publish only the recorded boundary"
    );
    assert_ne!(
        later_local_head.trim(),
        expected_boundary,
        "test setup must keep the later local commit outside the transported boundary"
    );
}

fn change_source(fixture: &Fixture, body: &str) {
    fs::write(fixture.root.path().join("skills/demo/details.txt"), body).expect("edit source");
}

fn convergence_operation_count(root: &Path) -> usize {
    common::operations_log(root)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|operation| operation["intent"] == json!("skill.converge"))
        .count()
}
