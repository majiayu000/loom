use std::fs;

use serde_json::{Value, json};

use super::*;
use crate::skill_convergence_executor::apply_plan;

fn convergence_operations(root: &Path) -> Vec<Value> {
    common::operations_log(root)
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("parse operation"))
        .filter(|operation| operation["intent"] == json!("skill.converge"))
        .collect()
}

#[test]
fn idempotent_replay_and_key_conflict() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "idempotent convergence\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let key = "raw-key-must-not-persist";
    let (output, first) = apply_plan(&fixture, &plan, key, &[]);
    assert!(output.status.success(), "first apply failed: {first}");
    let convergence_id = first["data"]["convergence_id"]
        .as_str()
        .expect("convergence id");
    assert!(convergence_id.starts_with("conv_"));
    assert_eq!(first["data"]["idempotent_replay"], json!(false));
    let first_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let first_operations = convergence_operations(fixture.root.path());
    assert_eq!(first_operations.len(), 1);
    assert_eq!(
        first_operations[0]["payload"]["convergence_id"],
        json!(convergence_id)
    );

    let (output, replay) = apply_plan(&fixture, &plan, key, &[]);
    assert!(output.status.success(), "replay failed: {replay}");
    assert_eq!(replay["data"]["idempotent_replay"], json!(true));
    assert_eq!(replay["data"]["convergence_id"], json!(convergence_id));
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), first_head);
    assert_eq!(convergence_operations(fixture.root.path()).len(), 1);

    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "different reviewed plan\n",
    )
    .expect("edit source for second plan");
    let (output, second_plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "second plan failed: {second_plan}");
    let head_before_conflict = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, conflict) = apply_plan(&fixture, &second_plan, key, &[]);
    assert!(!output.status.success(), "key conflict passed: {conflict}");
    assert_eq!(conflict["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        conflict["error"]["details"]["conflict"]["code"],
        json!("IDEMPOTENCY_KEY_REUSED")
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_before_conflict
    );
    assert_eq!(convergence_operations(fixture.root.path()).len(), 1);

    let audit = fs::read_to_string(fixture.root.path().join("state/events/commands.jsonl"))
        .expect("read command audit");
    let journal = fs::read_to_string(
        fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json"),
    )
    .expect("read transaction journal");
    assert!(!audit.contains(key), "raw key leaked to command audit");
    assert!(
        !journal.contains(key),
        "raw key leaked to transaction journal"
    );
}

#[test]
fn convergence_evidence_is_complete() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, applied) = apply_plan(&fixture, &plan, "evidence-key", &[]);
    assert!(output.status.success(), "apply failed: {applied}");

    let data = &applied["data"];
    let evidence = &data["evidence"];
    assert_eq!(data["plan_digest"], plan["data"]["plan_digest"]);
    assert!(
        data["idempotency_binding_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert_eq!(evidence["source"]["direction"], json!("source"));
    assert_eq!(evidence["projections"]["state"], json!("converged"));
    assert_eq!(
        evidence["projections"]["items"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(evidence["registry_operation"]["state"], json!("recorded"));
    assert_eq!(evidence["visibility"]["state"], json!("restart_required"));
    assert_eq!(evidence["remote"]["state"], json!("not_requested"));
    assert_eq!(evidence["recovery"]["state"], json!("journaled"));

    let operations = convergence_operations(fixture.root.path());
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["effects"], *evidence);
    assert_eq!(
        operations[0]["payload"]["convergence_id"],
        data["convergence_id"]
    );
}
