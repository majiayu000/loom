use std::fs;

use serde_json::json;

use super::*;
use crate::skill_convergence_executor::apply_plan;

#[cfg(unix)]
#[test]
fn materialize_symlink_digest_survives_registry_recovery() {
    use std::os::unix::fs::symlink;

    let fixture = projected_fixture_with_method("materialize");
    let source = fixture.root.path().join("skills/demo");
    fs::write(source.join("real.txt"), "materialized bytes\n").expect("real file");
    symlink("real.txt", source.join("alias.txt")).expect("contained source symlink");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "materialize plan failed: {plan}");
    assert_ne!(
        plan["data"]["effects"][0]["source_tree_digest"],
        plan["data"]["input"]["selected_input_tree_digest"]
    );
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "materialize-recovery",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_committing_registry",
        )],
    );
    assert!(
        !output.status.success(),
        "materialize did not stop: {interrupted}"
    );
    let (output, recovered) = apply_plan(&fixture, &plan, "materialize-recovery", &[]);
    assert!(
        output.status.success(),
        "materialize recovery failed: {recovered}"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/alias.txt"))
            .expect("materialized alias"),
        "materialized bytes\n"
    );
}

#[test]
fn recovery_rechecks_routing_before_projection_writes() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/recovery.txt"),
        "new projection bytes\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "routing-recovery",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_commit",
        )],
    );
    assert!(
        !output.status.success(),
        "source boundary did not stop: {interrupted}"
    );
    let target_before = snapshot_tree(fixture.target.path());
    let rules_path = fixture.root.path().join("state/registry/rules.json");
    let mut rules: Value =
        serde_json::from_slice(&fs::read(&rules_path).expect("rules")).expect("parse rules");
    rules["rules"] = json!([]);
    fs::write(
        &rules_path,
        serde_json::to_vec_pretty(&rules).expect("encode rules"),
    )
    .expect("remove live rule");

    let (output, rejected) = apply_plan(&fixture, &plan, "routing-recovery", &[]);
    assert!(
        !output.status.success(),
        "routing drift resumed: {rejected}"
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
}

#[test]
fn ignored_only_source_bytes_are_committed_and_projected() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join(".gitignore"),
        "skills/demo/ignored.txt\n",
    )
    .expect("gitignore");
    git(fixture.root.path(), &["add", ".gitignore"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: ignore convergence fixture"],
    );
    fs::write(
        fixture.root.path().join("skills/demo/ignored.txt"),
        "ignored but selected\n",
    )
    .expect("ignored skill file");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "ignored source plan failed: {plan}"
    );
    let (output, applied) = apply_plan(&fixture, &plan, "ignored-source", &[]);
    assert!(
        output.status.success(),
        "ignored source apply failed: {applied}"
    );
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        git(
            fixture.root.path(),
            &["show", &format!("{}:skills/demo/ignored.txt", head.trim())],
        ),
        "ignored but selected\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/ignored.txt"))
            .expect("projected ignored file"),
        "ignored but selected\n"
    );
}
