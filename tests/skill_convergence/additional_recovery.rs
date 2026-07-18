use std::fs;

use serde_json::json;

use super::*;
use crate::skill_convergence_executor::apply_plan;

#[test]
fn interrupted_projection_activation_recovers_refresh() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "projection activation recovery\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "projection-activation",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_projection_activation",
        )],
    );
    assert!(
        !output.status.success(),
        "activation did not stop: {interrupted}"
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "projection-activation", &[]);
    assert!(
        output.status.success(),
        "activation recovery failed: {recovered}"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/details.txt"))
            .expect("recovered projection"),
        "projection activation recovery\n"
    );
}

#[cfg(unix)]
#[test]
fn unregistered_safe_symlink_is_adopted_as_refresh() {
    let fixture = projected_fixture_with_method("symlink");
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&registry_path).expect("registry"))
        .expect("parse registry");
    registry["projections"] = json!([]);
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("encode registry"),
    )
    .expect("write registry");
    git(
        fixture.root.path(),
        &["add", "state/registry/projections.json"],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: remove safe symlink record"],
    );

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "adoption plan failed: {plan}");
    assert_eq!(plan["data"]["effects"][0]["effect"], json!("refresh"));
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "symlink-adoption",
        &[("LOOM_FAULT_INJECT", "convergence_interrupt_after_prepared")],
    );
    assert!(
        !output.status.success(),
        "adoption did not stop: {interrupted}"
    );
    let (output, recovered) = apply_plan(&fixture, &plan, "symlink-adoption", &[]);
    assert!(
        output.status.success(),
        "adoption recovery failed: {recovered}"
    );
    assert!(
        fs::symlink_metadata(fixture.target.path().join("demo"))
            .expect("adopted projection")
            .file_type()
            .is_symlink()
    );
}

#[test]
fn registry_cas_rejects_an_external_head_without_installing_index() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry cas source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "registry-cas",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_before_registry_cas",
        )],
    );
    assert!(
        !output.status.success(),
        "registry CAS did not stop: {interrupted}"
    );

    fs::write(fixture.root.path().join("external.txt"), "external\n").expect("external file");
    git(fixture.root.path(), &["add", "external.txt"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: external registry race"],
    );
    let external_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let index_before = fs::read(fixture.root.path().join(".git/index")).expect("index before");

    let (output, rejected) = apply_plan(&fixture, &plan, "registry-cas", &[]);
    assert!(
        !output.status.success(),
        "external HEAD was accepted: {rejected}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        external_head
    );
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).expect("index after"),
        index_before
    );
}
