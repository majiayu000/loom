use serde_json::{Value, json};

use super::convergence_test_sha256 as policy_input_sha256;
use super::*;

const CONVERGENCE_DIGEST_FIELDS: [&str; 14] = [
    "skill",
    "request_scope",
    "selectors",
    "source",
    "input",
    "preflight",
    "input_conflicts",
    "registry",
    "projections",
    "visibility",
    "accept_restart_required",
    "remote",
    "required_axes",
    "required_approvals",
];

fn reseal_projection_input_method(fixture: &Fixture, plan: &Value, method: &str) -> String {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let mut resealed = None;
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        let selected = stored["input"]["selected_projection_instance"]
            .as_str()
            .expect("selected projection")
            .to_string();
        let input = stored["input"]["projections"]
            .as_array_mut()
            .expect("projection inputs")
            .iter_mut()
            .find(|item| item["instance_id"].as_str() == Some(&selected))
            .expect("selected projection input");
        input["method"] = json!(method);
        let object = stored.as_object().expect("stored plan object");
        let payload = CONVERGENCE_DIGEST_FIELDS
            .into_iter()
            .map(|field| (field.to_string(), object[field].clone()))
            .collect::<serde_json::Map<_, _>>();
        let mut hasher = policy_input_sha256::Sha256::new();
        hasher.update(&serde_json::to_vec(&payload).expect("serialize digest payload"));
        let digest = format!("sha256:{}", policy_input_sha256::to_hex(&hasher.finalize()));
        stored["plan_digest"] = json!(digest);
        resealed = stored["plan_digest"].as_str().map(str::to_string);
    });
    resealed.expect("resealed plan digest")
}

#[test]
fn unsupported_projection_input_method_fails_closed_before_policy_capture() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("projection instance");
    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "projection plan failed: {plan}");
    let digest = reseal_projection_input_method(&fixture, &plan, "symlink");
    let before_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let before_source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let before_registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let before_target = snapshot_tree(fixture.target.path());

    let (output, rejected) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan["data"]["plan_id"].as_str().expect("plan id"),
            "--plan-digest",
            &digest,
            "--idempotency-key",
            "unsupported-policy-input-method",
        ],
    );
    assert!(
        !output.status.success(),
        "unsupported method applied: {rejected}"
    );
    assert_eq!(
        rejected["error"]["details"]["conflict"]["code"],
        json!("PLAN_POLICY_DRIFT")
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        before_head
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        before_source
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        before_registry
    );
    assert_eq!(snapshot_tree(fixture.target.path()), before_target);
}
