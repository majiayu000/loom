use std::{collections::BTreeSet, fs};

use serde_json::{Value, json};

use super::skill_convergence_executor::apply_plan;
use super::*;

#[test]
fn visibility_and_restart_states() {
    let fixture = projected_fixture();
    change_source(&fixture, "visibility reread\n");
    let (output, plan) = plan_converge(&fixture, &["--require-runtime"]);
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "visibility-state", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(data["local_state"], json!("complete"));
    assert_eq!(
        data["convergence"]["visibility"]["state"],
        json!("restart_required")
    );
    assert_eq!(data["complete"], json!(false));
    assert_eq!(data["outcome"], json!("local_complete_restart_required"));
    assert_eq!(
        data["completion_blockers"],
        json!(["visibility.restart_required"]),
        "unexpected blockers: {applied}"
    );
    assert_eq!(
        data["convergence"]["registry_transport"]["state"],
        json!("not_requested")
    );
    assert!(
        data["convergence"]["visibility"]["evidence"]["report"]["checks"]
            .as_array()
            .is_some_and(|checks| !checks.is_empty()),
        "visibility must come from an adapter reread: {applied}"
    );
}

#[test]
fn restart_required_acceptance_is_explicit() {
    let fixture = projected_fixture();
    change_source(&fixture, "accepted restart\n");
    let (output, plan) = plan_converge(
        &fixture,
        &["--require-runtime", "--accept-restart-required"],
    );
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["accept_restart_required"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "accepted-restart", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(
        data["convergence"]["visibility"]["state"],
        json!("restart_required")
    );
    assert_eq!(data["completion_blockers"], json!([]));
    assert_eq!(data["complete"], json!(true));
    assert_eq!(data["outcome"], json!("complete_with_restart_required"));
}

#[test]
fn remote_failure_preserves_local_completion() {
    let fixture = projected_fixture();
    let remote = common::TestDir::new("convergence-remote-retry");
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

    git(remote.path(), &["init", "--bare"]);
    let remote_path = remote.path().to_str().expect("remote path");
    git(
        fixture.root.path(),
        &["remote", "add", "origin", remote_path],
    );
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
    assert_eq!(
        retried_data["convergence"]["registry_transport"]["state"],
        json!("SYNCED")
    );
    assert_eq!(
        common::operations_log(fixture.root.path())
            .lines()
            .filter(|line| line.contains("\"intent\":\"skill.converge\""))
            .count(),
        1,
        "remote retry duplicated the aggregate operation"
    );
}

#[test]
fn remote_pending_and_restart_blockers_compose() {
    let fixture = projected_fixture();
    change_source(&fixture, "two independent blockers\n");
    let (output, plan) = plan_converge(&fixture, &["--push-remote", "--require-runtime"]);
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, applied) = apply_plan(&fixture, &plan, "combined-blockers", &[]);
    assert!(output.status.success(), "partial apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(
        data["outcome"],
        json!("local_complete_remote_pending_restart_required")
    );
    assert_eq!(
        data["completion_blockers"],
        json!(["registry.remote_pending", "visibility.restart_required"])
    );
    assert_eq!(data["next_actions"].as_array().map(Vec::len), Some(2));
    assert!(
        data["next_actions"][0]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("transport"))
    );
    assert!(
        data["next_actions"][1]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("restart"))
    );
}

#[test]
fn complete_requires_declared_evidence() {
    let fixture = projected_fixture();
    rewrite_fixture_agent(&fixture, "cursor");
    change_source(&fixture, "unsupported visibility evidence\n");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, plan) = common::run_loom(
        fixture.root.path(),
        &[
            "plan",
            "converge",
            "demo",
            "--agent",
            "cursor",
            "--workspace",
            workspace,
            "--profile",
            "default",
            "--require-runtime",
        ],
    );
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, applied) = apply_plan(&fixture, &plan, "missing-evidence", &[]);
    assert!(output.status.success(), "partial apply failed: {applied}");
    let data = &applied["data"];
    assert_eq!(
        data["convergence"]["visibility"]["state"],
        json!("unsupported")
    );
    assert_eq!(data["complete"], json!(false));
    assert_eq!(
        data["completion_blockers"],
        json!(["visibility.evidence_incomplete"])
    );
    assert_eq!(data["outcome"], json!("local_complete_evidence_incomplete"));
}

#[test]
fn complete_requires_the_exact_planned_projection_set() {
    let fixture = projected_fixture();
    let (_, second_instance) = add_copy_projection(&fixture, "second-post-local-target");
    change_source(&fixture, "two exact projection effects\n");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["effects"].as_array().map(Vec::len), Some(2));

    let planned_ids = plan["data"]["effects"]
        .as_array()
        .expect("planned effects")
        .iter()
        .map(|effect| effect["instance_id"].as_str().expect("planned instance"))
        .collect::<BTreeSet<_>>();
    assert!(planned_ids.contains(second_instance.as_str()));

    let (output, applied) = apply_plan(&fixture, &plan, "exact-projection-set", &[]);
    assert!(output.status.success(), "apply failed: {applied}");
    let projections = &applied["data"]["convergence"]["projections"];
    assert_eq!(applied["data"]["complete"], json!(true));
    assert_eq!(projections["evidence"]["selected_count"], json!(2));
    let observed_ids = projections["items"]
        .as_array()
        .expect("projection evidence")
        .iter()
        .map(|item| item["instance_id"].as_str().expect("observed instance"))
        .collect::<BTreeSet<_>>();
    assert_eq!(observed_ids, planned_ids);
}

fn change_source(fixture: &Fixture, body: &str) {
    fs::write(fixture.root.path().join("skills/demo/details.txt"), body).expect("edit source");
}

fn rewrite_fixture_agent(fixture: &Fixture, agent: &str) {
    for (file, key) in [("targets.json", "targets"), ("bindings.json", "bindings")] {
        let path = fixture.root.path().join("state/registry").join(file);
        let mut value: Value =
            serde_json::from_slice(&fs::read(&path).expect("read registry file"))
                .expect("parse registry file");
        for row in value[key].as_array_mut().expect("registry rows") {
            row["agent"] = json!(agent);
        }
        fs::write(
            &path,
            serde_json::to_vec_pretty(&value).expect("encode registry file"),
        )
        .expect("rewrite registry file");
    }
    git(fixture.root.path(), &["add", "state/registry"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: use generic visibility adapter"],
    );
}
