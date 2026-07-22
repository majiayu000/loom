use common::operations_log;
use serde_json::json;

use super::skill_convergence_executor::apply_plan;
use super::*;

fn replace_prior_apply_binding(root: &Path, replacement: Option<Value>) {
    let path = root.join("state/events/commands.jsonl");
    let raw = std::fs::read_to_string(&path).expect("read command events");
    let mut replaced = false;
    let lines = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).expect("parse command event");
            if !replaced && event["cmd"] == json!("apply") && event["status"] == json!("succeeded")
            {
                let output = event["output"]
                    .as_object_mut()
                    .expect("apply output object");
                match replacement.clone() {
                    Some(value) => {
                        output.insert("idempotency_binding_digest".to_string(), value);
                    }
                    None => {
                        output.remove("idempotency_binding_digest");
                    }
                }
                replaced = true;
            }
            serde_json::to_string(&event).expect("serialize command event")
        })
        .collect::<Vec<_>>();
    assert!(replaced, "expected a prior succeeded apply event");
    std::fs::write(path, format!("{}\n", lines.join("\n"))).expect("rewrite command events");
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
}

#[test]
fn convergence_replay_rejects_unbound_prior_event() {
    for (case, replacement) in [
        ("missing", None),
        ("non-string", Some(json!({ "invalid": true }))),
    ] {
        let fixture = projected_fixture();
        std::fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("unbound replay {case}\n"),
        )
        .expect("edit source");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed for {case}: {plan}");
        let key = format!("unbound-{case}");
        let (output, first) = apply_plan(&fixture, &plan, &key, &[]);
        assert!(
            output.status.success(),
            "first apply failed for {case}: {first}"
        );
        let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let tree = snapshot_tree(fixture.target.path());

        replace_prior_apply_binding(fixture.root.path(), replacement);
        let (output, rejected) = apply_plan(&fixture, &plan, &key, &[]);
        assert!(
            !output.status.success(),
            "unbound prior event replayed for {case}: {rejected}"
        );
        assert_eq!(rejected["error"]["code"], json!("DEPENDENCY_CONFLICT"));
        assert_eq!(
            rejected["error"]["details"]["conflict"]["code"],
            json!("IDEMPOTENCY_BINDING_MISMATCH")
        );
        assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(snapshot_tree(fixture.target.path()), tree);
    }
}

#[test]
fn interrupted_recovery_rejects_a_different_idempotency_key() {
    let fixture = projected_fixture();
    std::fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "interrupted identity\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "original-key",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_commit",
        )],
    );
    assert!(
        !output.status.success(),
        "apply did not stop: {interrupted}"
    );
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let tree = snapshot_tree(fixture.target.path());
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal = std::fs::read(&journal_path).expect("interrupted journal");
    let journal_value: Value = serde_json::from_slice(&journal).expect("parse interrupted journal");
    let convergence_id = journal_value["convergence_id"]
        .as_str()
        .expect("persisted convergence id")
        .to_string();
    assert_eq!(journal_value["plan_digest"], plan["data"]["plan_digest"]);
    assert!(journal_value["idempotency_key_digest"].is_string());
    assert!(journal_value["idempotency_binding_digest"].is_string());

    let (output, rejected) = apply_plan(&fixture, &plan, "different-key", &[]);
    assert!(
        !output.status.success(),
        "different key resumed the transaction: {rejected}"
    );
    assert_eq!(rejected["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        rejected["error"]["details"]["conflict"]["code"],
        json!("IDEMPOTENCY_BINDING_MISMATCH")
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(snapshot_tree(fixture.target.path()), tree);
    assert_eq!(
        std::fs::read(&journal_path).expect("preserved journal"),
        journal,
        "identity rejection must not mutate recovery evidence"
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "original-key", &[]);
    assert!(
        output.status.success(),
        "owner recovery failed: {recovered}"
    );
    assert_eq!(recovered["data"]["convergence_id"], json!(convergence_id));
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
    let operations_before_apply = operations_log(fixture.root.path());

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
    assert_eq!(
        applied_evidence["registry_operation"],
        json!({
            "state": "not_applicable",
            "reason": "convergence_mode",
        }),
        "convergence must distinguish an inapplicable registry operation from missing evidence"
    );
    assert_eq!(
        operations_log(fixture.root.path()),
        operations_before_apply,
        "convergence apply must not append a registry ops ledger row"
    );

    // The terminal journal retains the same local evidence after commit.
    let journal: Value = serde_json::from_str(
        &std::fs::read_to_string(
            fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json"),
        )
        .expect("retained convergence journal"),
    )
    .expect("parse retained convergence journal");
    assert_eq!(journal["phase"], json!("committed_artifacts_retained"));
    assert_eq!(journal["plan_id"], json!(plan_id));
    assert_eq!(
        journal["result"]["registry_operation"], applied_evidence["registry_operation"],
        "retained journal and result envelope must agree on registry operation applicability"
    );

    // The durable command event makes the result envelope discoverable by convergence_id.
    let events = std::fs::read_to_string(fixture.root.path().join("state/events/commands.jsonl"))
        .expect("command events");
    let persisted_apply = events
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| {
            event["cmd"] == json!("apply")
                && event["status"] == json!("succeeded")
                && event["output"]["convergence_id"] == json!(convergence_id)
        })
        .expect("persisted convergence apply envelope");
    assert_eq!(
        persisted_apply["output"]["applied"]["registry_operation"],
        applied_evidence["registry_operation"]
    );

    // The raw idempotency key must never appear in any persisted state.
    assert!(
        !events.contains("evidence-key"),
        "raw idempotency key leaked into the command event log"
    );
    assert!(
        !operations_log(fixture.root.path()).contains("evidence-key"),
        "raw idempotency key leaked into the operations log"
    );
}
