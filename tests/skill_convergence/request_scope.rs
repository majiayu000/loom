use serde_json::{Value, json};

use super::convergence_test_sha256 as request_scope_sha256;
use super::*;

const CONVERGENCE_DIGEST_FIELDS: [&str; 13] = [
    "skill",
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

fn reseal_plan_event(
    fixture: &Fixture,
    plan: &Value,
    mut mutate: impl FnMut(&mut Value),
) -> String {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let mut resealed = None;
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        mutate(stored);
        let object = stored.as_object().expect("stored plan object");
        let payload = CONVERGENCE_DIGEST_FIELDS
            .into_iter()
            .map(|field| (field.to_string(), object[field].clone()))
            .collect::<serde_json::Map<_, _>>();
        let mut hasher = request_scope_sha256::Sha256::new();
        hasher.update(&serde_json::to_vec(&payload).expect("serialize plan digest payload"));
        let digest = format!(
            "sha256:{}",
            request_scope_sha256::to_hex(&hasher.finalize())
        );
        stored["plan_digest"] = json!(digest);
        resealed = stored["plan_digest"].as_str().map(str::to_string);
    });
    resealed.expect("resealed plan digest")
}

fn apply_resealed_plan(
    fixture: &Fixture,
    plan: &Value,
    digest: &str,
    key: &str,
) -> (std::process::Output, Value) {
    run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan["data"]["plan_id"].as_str().expect("plan id"),
            "--plan-digest",
            digest,
            "--idempotency-key",
            key,
        ],
    )
}

fn assert_request_scope_rejected(output: std::process::Output, envelope: &Value) {
    assert!(
        !output.status.success(),
        "resealed request scope applied: {envelope}"
    );
    assert_eq!(
        envelope["error"]["details"]["conflict"]["code"],
        json!("PLAN_REQUEST_SCOPE_DRIFT")
    );
}

#[test]
fn runtime_requirement_is_bound_to_the_started_plan_request() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(
        &fixture,
        &["--require-runtime", "--accept-restart-required"],
    );
    assert!(output.status.success(), "runtime plan failed: {plan}");

    let digest = reseal_plan_event(&fixture, &plan, |stored| {
        stored["required_axes"] = json!(["projections"]);
        stored["accept_restart_required"] = json!(false);
        for item in stored["visibility"]
            .as_array_mut()
            .expect("visibility requirements")
        {
            item["required"] = json!(false);
        }
    });
    let (output, envelope) = apply_resealed_plan(&fixture, &plan, &digest, "runtime-request-scope");
    assert_request_scope_rejected(output, &envelope);
}

#[test]
fn remote_policy_is_bound_to_the_started_plan_request() {
    for (args, remote, axes, key) in [
        (
            Vec::<&str>::new(),
            "push",
            json!(["projections", "registry_transport"]),
            "remote-request-added",
        ),
        (
            vec!["--push-remote"],
            "not_requested",
            json!(["projections"]),
            "remote-request-removed",
        ),
    ] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &args);
        assert!(output.status.success(), "remote plan failed: {plan}");

        let digest = reseal_plan_event(&fixture, &plan, |stored| {
            stored["remote"] = json!(remote);
            stored["required_axes"] = axes.clone();
        });
        let (output, envelope) = apply_resealed_plan(&fixture, &plan, &digest, key);
        assert_request_scope_rejected(output, &envelope);
    }
}
