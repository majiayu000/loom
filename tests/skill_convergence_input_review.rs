mod common;

use std::fs;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};

#[path = "../src/sha256.rs"]
mod sha256;

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
use serde_json::{Value, json};

struct Fixture {
    root: TestDir,
    workspace: TestDir,
    target: TestDir,
}

fn projected_fixture() -> Fixture {
    projected_fixture_with_method("copy")
}

fn projected_fixture_with_method(method: &str) -> Fixture {
    let root = TestDir::new("convergence-input-review-root");
    let workspace = TestDir::new("convergence-input-review-workspace");
    let target = TestDir::new("convergence-input-review-target");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing convergence input review.\n---\n# demo\n",
    );
    let (output, env) = save_skill(root.path(), "demo");
    assert!(output.status.success(), "save skill failed: {env}");
    let (output, env) = target_add(root.path(), "claude", target.path(), "managed");
    assert!(output.status.success(), "target add failed: {env}");
    let target_id = env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (output, env) = binding_add(
        root.path(),
        "claude",
        "default",
        "exact-path",
        workspace_arg,
        target_id,
    );
    assert!(output.status.success(), "binding add failed: {env}");
    let binding_id = env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (output, env) = skill_project(root.path(), "demo", binding_id, Some(method));
    assert!(output.status.success(), "project failed: {env}");
    Fixture {
        root,
        workspace,
        target,
    }
}

fn plan_converge(fixture: &Fixture, extra: &[&str]) -> (std::process::Output, Value) {
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let mut args = vec![
        "plan",
        "converge",
        "demo",
        "--agent",
        "claude",
        "--workspace",
        workspace,
        "--profile",
        "default",
    ];
    args.extend_from_slice(extra);
    run_loom(fixture.root.path(), &args)
}

fn conflict_codes(plan: &Value) -> Vec<&str> {
    plan["data"]["conflicts"]
        .as_array()
        .expect("conflicts")
        .iter()
        .filter_map(|conflict| conflict["code"].as_str())
        .collect()
}

fn mutate_plan_event(root: &std::path::Path, plan_id: &str, mutate: impl FnOnce(&mut Value)) {
    let audit_path = root.join("state/events/commands.jsonl");
    let raw = fs::read_to_string(&audit_path).expect("read command events");
    let mut mutate = Some(mutate);
    let rows = raw
        .lines()
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).expect("parse command event");
            let stored_plan = event.get("durable_plan").unwrap_or(&event["output"]);
            if event["cmd"] == json!("plan.converge")
                && event["status"] == json!("succeeded")
                && stored_plan["plan_id"] == json!(plan_id)
                && let Some(mutate) = mutate.take()
            {
                let stored_plan = if event.get("durable_plan").is_some() {
                    &mut event["durable_plan"]
                } else {
                    &mut event["output"]
                };
                mutate(stored_plan);
            }
            serde_json::to_string(&event).expect("serialize command event")
        })
        .collect::<Vec<_>>();
    assert!(mutate.is_none(), "expected stored convergence plan event");
    fs::write(&audit_path, format!("{}\n", rows.join("\n"))).expect("rewrite stored plan");
}

fn raw_convergence_digest(plan: &Value) -> String {
    let plan = plan.as_object().expect("plan object");
    let payload = CONVERGENCE_DIGEST_FIELDS
        .into_iter()
        .map(|field| (field.to_string(), plan[field].clone()))
        .collect::<serde_json::Map<_, _>>();
    let mut hasher = sha256::Sha256::new();
    hasher.update(&serde_json::to_vec(&payload).expect("serialize digest payload"));
    format!("sha256:{}", sha256::to_hex(&hasher.finalize()))
}

#[test]
fn projection_input_drives_policy_approvals_and_risks() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    assert_eq!(initial["data"]["required_approvals"], json!([]));

    fs::write(
        fixture.target.path().join("demo/SKILL.md"),
        r#"---
name: demo
description: Use when testing projection policy evidence.
capabilities:
  shell:
    commands: ["git"]
  network:
    domains: ["api.example.com"]
---
# demo
"#,
    )
    .expect("write risky projection input");

    let (output, projection) =
        plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "projection plan failed: {projection}"
    );
    assert_eq!(
        projection["data"]["required_approvals"],
        json!(["network", "policy-high-risk", "shell"])
    );
    let risk_codes = projection["data"]["risks"]
        .as_array()
        .expect("risks")
        .iter()
        .filter_map(|risk| risk["code"].as_str())
        .collect::<Vec<_>>();
    assert!(risk_codes.contains(&"capability_shell_commands"));
    assert!(risk_codes.contains(&"capability_network_domains"));
    assert_ne!(
        projection["data"]["plan_digest"],
        initial["data"]["plan_digest"]
    );
    assert!(
        projection["data"]["effects"]
            .as_array()
            .expect("effects")
            .iter()
            .all(|effect| effect["source_tree_digest"]
                == projection["data"]["input"]["selected_input_tree_digest"])
    );

    let (output, source) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "source plan failed: {source}");
    assert_eq!(source["data"]["required_approvals"], json!([]));
}

#[test]
fn stale_projection_baseline_is_reported_for_projection_input() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    write_skill(
        fixture.root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing a newer canonical source.\n---\n# newer source\n",
    );
    let (output, saved) = save_skill(fixture.root.path(), "demo");
    assert!(output.status.success(), "save newer source failed: {saved}");

    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "stale input plan failed: {plan}");
    assert!(conflict_codes(&plan).contains(&"STALE_PROJECTION_INPUT"));
}

#[test]
fn untracked_existing_live_path_fails_closed() {
    let fixture = projected_fixture();
    let projections_path = fixture.root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_slice(&fs::read(&projections_path).expect("read projections"))
            .expect("parse projections");
    projections["projections"] = json!([]);
    fs::write(
        &projections_path,
        serde_json::to_vec_pretty(&projections).expect("serialize projections"),
    )
    .expect("remove projection record");

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "untracked path plan failed: {plan}"
    );
    assert!(conflict_codes(&plan).contains(&"PROJECTION_EVIDENCE_UNAVAILABLE"));
    assert_eq!(
        plan["data"]["input"]["projections"][0]["issue"],
        json!("unmanaged_live_path_without_projection_record")
    );
}

#[test]
fn non_directory_projection_path_fails_closed_for_source_plan() {
    let fixture = projected_fixture();
    fs::remove_dir_all(fixture.target.path().join("demo")).expect("remove projection directory");
    fs::write(fixture.target.path().join("demo"), "unmanaged bytes\n")
        .expect("replace projection with file");

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "non-directory plan failed: {plan}");
    assert!(conflict_codes(&plan).contains(&"PROJECTION_EVIDENCE_UNAVAILABLE"));
}

#[test]
fn stored_schema_1_1_convergence_plan_reports_migration() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        stored["schema_version"] = json!("1.1");
        let object = stored.as_object_mut().expect("stored plan object");
        object.remove("input");
        object.remove("preflight");
        object.remove("input_conflicts");
    });

    let (output, applied) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "schema-1-1-compat",
        ],
    );
    assert!(
        !output.status.success(),
        "legacy schema unexpectedly ran: {applied}"
    );
    assert_eq!(applied["error"]["code"], json!("SCHEMA_MISMATCH"));
    assert_eq!(
        applied["error"]["details"]["conflict"]["code"],
        json!("PLAN_SCHEMA_UNSUPPORTED")
    );
}

#[test]
fn stored_schema_1_2_without_request_scope_reports_migration() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        stored["schema_version"] = json!("1.2");
        stored
            .as_object_mut()
            .expect("stored plan object")
            .remove("request_scope");
    });

    let (output, applied) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "schema-1-2-compat",
        ],
    );
    assert!(!output.status.success(), "legacy schema ran: {applied}");
    assert_eq!(applied["error"]["code"], json!("SCHEMA_MISMATCH"));
    assert_eq!(
        applied["error"]["details"]["conflict"]["code"],
        json!("PLAN_SCHEMA_UNSUPPORTED")
    );
}

#[test]
fn stored_shape_corruption_is_rejected_even_with_matching_digest() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let mut corrupt_digest = None;
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        stored["input_conflicts"] = json!("invalid-array-shape");
        let digest = raw_convergence_digest(stored);
        stored["plan_digest"] = json!(digest);
        corrupt_digest = stored["plan_digest"].as_str().map(str::to_string);
    });
    let corrupt_digest = corrupt_digest.expect("corrupt digest");

    let (output, rejected) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            &corrupt_digest,
            "--idempotency-key",
            "conv-shape-corrupt",
        ],
    );
    assert!(!output.status.success(), "corrupt shape passed: {rejected}");
    assert_eq!(rejected["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        rejected["error"]["details"]["conflict"]["code"],
        json!("PLAN_CORRUPT")
    );
}

#[test]
fn working_source_drift_blocks_clean_projection_input() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    fs::write(
        fixture.root.path().join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Use when testing working source drift.\n---\n# dirty source\n",
    )
    .expect("write dirty source");

    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "working drift plan failed: {plan}");
    assert!(conflict_codes(&plan).contains(&"STALE_PROJECTION_INPUT"));
}

#[test]
fn unavailable_baseline_remains_a_reviewable_conflict() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    let projections_path = fixture.root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_slice(&fs::read(&projections_path).expect("read projections"))
            .expect("parse projections");
    projections["projections"][0]["last_applied_rev"] = json!("missing-baseline-ref");
    fs::write(
        &projections_path,
        serde_json::to_vec_pretty(&projections).expect("serialize projections"),
    )
    .expect("write missing baseline");

    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "unavailable baseline plan failed: {plan}"
    );
    assert!(conflict_codes(&plan).contains(&"PROJECTION_EVIDENCE_UNAVAILABLE"));
}

#[test]
fn nested_git_metadata_is_excluded_from_projection_input_digest() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    let metadata = fixture.target.path().join("demo/.git");
    fs::create_dir_all(&metadata).expect("create nested git metadata");
    fs::write(
        metadata.join("config"),
        "[core]\n\trepositoryformatversion = 0\n",
    )
    .expect("write nested git metadata");

    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "nested git metadata blocked plan: {plan}"
    );
    assert_eq!(
        plan["data"]["input"]["selected_input_tree_digest"],
        initial["data"]["source"]["tree_digest"]
    );
}

#[test]
fn dirty_projection_requires_explicit_input_selection() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    fs::write(
        fixture.target.path().join("demo/SKILL.md"),
        "---\nname: demo\ndescription: Use when testing one dirty projection.\n---\n# dirty\n",
    )
    .expect("write dirty projection");

    let (output, source) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "source plan failed: {source}");
    assert!(conflict_codes(&source).contains(&"DIRTY_PROJECTION_INPUT_REQUIRES_SELECTION"));

    let (output, selected) =
        plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "selected plan failed: {selected}");
    assert!(!conflict_codes(&selected).contains(&"DIRTY_PROJECTION_INPUT_REQUIRES_SELECTION"));
}

#[cfg(unix)]
#[test]
fn copy_projection_input_preserves_contained_symlinks_for_preflight() {
    use std::os::unix::fs::symlink;

    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    let live = fixture.target.path().join("demo");
    fs::write(live.join("details.txt"), "contained\n").expect("write contained target");
    symlink("details.txt", live.join("current.txt")).expect("create contained symlink");

    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "contained symlink plan failed: {plan}"
    );
    assert_eq!(
        plan["data"]["preflight"]["input_direction"],
        json!("projection")
    );
}

#[cfg(unix)]
#[test]
fn projection_input_rejects_unsafe_or_materialized_symlinks() {
    use std::os::unix::fs::symlink;

    let copy = projected_fixture();
    let (output, plan) = plan_converge(&copy, &[]);
    assert!(output.status.success(), "copy plan failed: {plan}");
    let instance = plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("copy instance");
    symlink("../outside", copy.target.path().join("demo/escape")).expect("escaping symlink");
    let (output, rejected) = plan_converge(&copy, &["--from-projection", "--instance", instance]);
    assert!(
        !output.status.success(),
        "escaping symlink passed: {rejected}"
    );

    let materialize = projected_fixture_with_method("materialize");
    let (output, plan) = plan_converge(&materialize, &[]);
    assert!(output.status.success(), "materialize plan failed: {plan}");
    let instance = plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("materialize instance");
    fs::write(
        materialize.target.path().join("demo/details.txt"),
        "details\n",
    )
    .expect("materialize target");
    symlink(
        "details.txt",
        materialize.target.path().join("demo/current.txt"),
    )
    .expect("materialize symlink");
    let (output, rejected) =
        plan_converge(&materialize, &["--from-projection", "--instance", instance]);
    assert!(
        !output.status.success(),
        "materialize symlink input passed: {rejected}"
    );
}

#[test]
fn selected_agent_scopes_projection_preflight_dependencies() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {plan}");
    let instance = plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    let agents = fixture.target.path().join("demo/agents");
    fs::create_dir_all(&agents).expect("create agent metadata");
    fs::write(
        agents.join("codex.yaml"),
        "requires_tools: loom-selector-tool-that-does-not-exist\n",
    )
    .expect("write unrelated agent dependency");

    let (output, selected) =
        plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "selected agent plan failed: {selected}"
    );
    assert!(
        !conflict_codes(&selected).contains(&"SOURCE_PREFLIGHT_BLOCKED"),
        "claude preflight must not include codex-only dependencies: {selected}"
    );
}

#[test]
fn derived_runtime_agent_scopes_projection_preflight_dependencies() {
    let fixture = projected_fixture();
    let agents = fixture.root.path().join("skills/demo/agents");
    fs::create_dir_all(&agents).expect("create agent metadata");
    fs::write(
        agents.join("codex.yaml"),
        "requires_tools: loom-selector-tool-that-does-not-exist\n",
    )
    .expect("write unrelated agent dependency");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");

    let (output, plan) = run_loom(
        fixture.root.path(),
        &[
            "plan",
            "converge",
            "demo",
            "--workspace",
            workspace,
            "--profile",
            "default",
            "--require-runtime",
        ],
    );
    assert!(output.status.success(), "derived agent plan failed: {plan}");
    assert_eq!(plan["data"]["selectors"]["agent"], json!("claude"));
    assert!(
        !conflict_codes(&plan).contains(&"SOURCE_PREFLIGHT_BLOCKED"),
        "derived claude preflight must not include codex-only dependencies: {plan}"
    );
}
