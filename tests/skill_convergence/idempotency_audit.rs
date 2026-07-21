use serde_json::json;

use super::skill_convergence_executor::apply_plan;
use super::*;
use common::operations_log;

fn converge_operations(root: &Path) -> Vec<Value> {
    operations_log(root)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<Value>(line).expect("operation record"))
        .filter(|record| record["intent"] == json!("skill.converge"))
        .collect()
}

#[test]
fn idempotent_replay_and_key_conflict() {
    let fixture = projected_fixture();
    std::fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "idempotent source\n",
    )
    .expect("edit source");

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, first) = apply_plan(&fixture, &plan, "shared-key", &[]);
    assert!(output.status.success(), "first apply failed: {first}");
    assert_eq!(first["data"]["idempotent_replay"], json!(false));

    let convergence_id = first["data"]["convergence_id"]
        .as_str()
        .expect("convergence id")
        .to_string();
    let head_after_first = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let tree_after_first = snapshot_tree(fixture.target.path());
    let converge_ops_after_first = converge_operations(fixture.root.path());
    assert_eq!(
        converge_ops_after_first.len(),
        1,
        "first apply must write exactly one aggregate converge operation"
    );

    // Same key + same plan must replay the recorded result with zero new side effects.
    let (output, replay) = apply_plan(&fixture, &plan, "shared-key", &[]);
    assert!(output.status.success(), "replay failed: {replay}");
    assert_eq!(replay["data"]["idempotent_replay"], json!(true));
    assert_eq!(
        replay["data"]["convergence_id"],
        json!(convergence_id),
        "replay must reuse the original convergence id"
    );
    assert_eq!(
        replay["data"]["applied"], first["data"]["applied"],
        "replay must return the recorded applied evidence"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_after_first,
        "replay must not create a new commit"
    );
    assert_eq!(
        snapshot_tree(fixture.target.path()),
        tree_after_first,
        "replay must not re-run a projection swap"
    );
    assert_eq!(
        converge_operations(fixture.root.path()),
        converge_ops_after_first,
        "replay must not append another aggregate operation record"
    );

    // Same key against a different plan must fail closed.
    let (output, second_plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "second plan failed: {second_plan}");
    assert_ne!(
        second_plan["data"]["plan_id"], plan["data"]["plan_id"],
        "second plan must be a distinct durable plan"
    );

    let (output, conflict) = apply_plan(&fixture, &second_plan, "shared-key", &[]);
    assert!(
        !output.status.success(),
        "reusing a key across plans must fail: {conflict}"
    );
    assert_eq!(conflict["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        conflict["error"]["details"]["conflict"]["code"],
        json!("IDEMPOTENCY_KEY_REUSED")
    );
    assert_eq!(
        converge_operations(fixture.root.path()),
        converge_ops_after_first,
        "a blocked key reuse must not write an operation record"
    );
}

#[test]
fn convergence_evidence_is_complete() {
    let fixture = projected_fixture();
    std::fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "evidence source\n",
    )
    .expect("edit source");

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let plan_digest = plan["data"]["plan_digest"].as_str().expect("plan digest");

    let (output, applied) = apply_plan(&fixture, &plan, "evidence-key", &[]);
    assert!(output.status.success(), "apply failed: {applied}");

    let data = &applied["data"];
    let convergence_id = data["convergence_id"].as_str().expect("convergence id");
    assert!(
        convergence_id.starts_with("conv_"),
        "convergence id must be namespaced: {convergence_id}"
    );
    assert_eq!(data["plan_id"], json!(plan_id));
    assert_eq!(
        data["plan_digest"],
        json!(plan_digest),
        "apply evidence must carry the confirmed plan digest"
    );

    // The idempotency binding must be derived from key + plan id + plan digest,
    // and the raw key must never be persisted.
    let key_digest = data["idempotency_key_digest"].as_str().expect("key digest");
    let binding_digest = data["idempotency_binding_digest"]
        .as_str()
        .expect("binding digest");
    assert!(key_digest.starts_with("sha256:"));
    assert!(binding_digest.starts_with("sha256:"));
    assert_ne!(
        key_digest, binding_digest,
        "the binding digest must not equal the bare key digest"
    );

    // Per-axis evidence.
    let applied_evidence = &data["applied"];
    assert_eq!(applied_evidence["skill"], json!("demo"));
    assert!(
        applied_evidence["source_commit"].is_string(),
        "source commit evidence missing: {applied_evidence}"
    );
    assert!(
        applied_evidence["projection_instances"].is_array(),
        "projection evidence missing: {applied_evidence}"
    );

    // Exactly one aggregate operation record, bound to this convergence.
    let ops = converge_operations(fixture.root.path());
    assert_eq!(
        ops.len(),
        1,
        "expected exactly one aggregate record: {ops:?}"
    );
    let record = &ops[0];
    assert_eq!(record["payload"]["convergence_id"], json!(convergence_id));
    assert_eq!(record["payload"]["plan_id"], json!(plan_id));
    assert_eq!(record["payload"]["plan_digest"], json!(plan_digest));
    assert_eq!(
        record["payload"]["idempotency_binding_digest"],
        json!(binding_digest)
    );
    assert_eq!(
        record["effects"]["source_commit"],
        applied_evidence["source_commit"]
    );

    // The aggregate op id must be reachable from the envelope meta.
    assert_eq!(
        applied["meta"]["op_id"], record["op_id"],
        "envelope meta must surface the aggregate operation id"
    );

    // The raw idempotency key must never appear in any persisted state.
    let events = std::fs::read_to_string(fixture.root.path().join("state/events/commands.jsonl"))
        .expect("command events");
    assert!(
        !events.contains("evidence-key"),
        "raw idempotency key leaked into the command event log"
    );
    assert!(
        !operations_log(fixture.root.path()).contains("evidence-key"),
        "raw idempotency key leaked into the operations log"
    );
}
