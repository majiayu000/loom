mod common;

use std::fs;

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};
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
