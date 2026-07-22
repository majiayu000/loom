use serde_json::{Value, json};

use super::convergence_test_sha256 as request_scope_sha256;
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
fn complete_request_scope_is_digest_covered() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(
        &fixture,
        &[
            "--require-runtime",
            "--accept-restart-required",
            "--push-remote",
        ],
    );
    assert!(output.status.success(), "plan failed: {plan}");

    assert_eq!(
        plan["data"]["request_scope"],
        json!({
            "skill": "demo",
            "direction": "source",
            "instance": null,
            "agent": "claude",
            "workspace_argument": fixture.workspace.path().display().to_string(),
            "workspace": fs::canonicalize(fixture.workspace.path())
                .expect("canonical workspace")
                .display()
                .to_string(),
            "profile": "default",
            "require_runtime": true,
            "accept_restart_required": true,
            "push_remote": true,
        })
    );
}

#[test]
fn relative_workspace_request_survives_apply_from_a_different_cwd() {
    let fixture = projected_fixture();
    let planning_cwd = fixture.workspace.path().parent().expect("workspace parent");
    let relative_workspace = fixture
        .workspace
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("relative workspace");
    let (output, plan) = common::run_loom_in_cwd(
        fixture.root.path(),
        planning_cwd,
        &[
            "plan",
            "converge",
            "demo",
            "--agent",
            "claude",
            "--workspace",
            relative_workspace,
            "--profile",
            "default",
        ],
    );
    assert!(output.status.success(), "relative plan failed: {plan}");
    assert_eq!(
        plan["data"]["request_scope"]["workspace_argument"],
        json!(relative_workspace)
    );
    assert_eq!(
        plan["data"]["request_scope"]["workspace"],
        json!(
            fs::canonicalize(fixture.workspace.path())
                .expect("canonical workspace")
                .display()
                .to_string()
        )
    );

    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    let (output, applied) = common::run_loom_in_cwd(
        fixture.root.path(),
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "relative-workspace-cross-cwd",
        ],
    );
    assert!(
        output.status.success(),
        "reviewed relative workspace drifted across cwd: {applied}"
    );
}

#[test]
fn workspace_argument_and_normalized_binding_are_both_request_bound() {
    for (field, replacement, key) in [
        (
            "workspace_argument",
            "different-relative-workspace",
            "workspace-argument-drift",
        ),
        (
            "workspace",
            "/tmp/different-normalized-workspace",
            "workspace-binding-drift",
        ),
    ] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let digest = reseal_plan_event(&fixture, &plan, |stored| {
            stored["request_scope"][field] = json!(replacement);
        });

        let (output, envelope) = apply_resealed_plan(&fixture, &plan, &digest, key);
        assert_request_scope_rejected(output, &envelope);
    }
}

#[test]
fn normalized_workspace_cannot_be_self_attested_by_the_resealed_plan() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let replacement = "/tmp/different-normalized-workspace";
    let digest = reseal_plan_event(&fixture, &plan, |stored| {
        stored["request_scope"]["workspace"] = json!(replacement);
        stored["selectors"]["workspace"] = json!(replacement);
    });

    let (output, envelope) =
        apply_resealed_plan(&fixture, &plan, &digest, "workspace-self-attestation-drift");
    assert_request_scope_rejected(output, &envelope);
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
