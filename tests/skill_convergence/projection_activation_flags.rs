use std::fs;

use serde_json::{Value, json};

use super::*;
use crate::skill_convergence_executor::{apply_plan, assert_exact_retained_ledger};

fn add_test_projection(fixture: &Fixture, suffix: &str, method: &str) {
    let target_path = fixture.root.path().join(format!("activation/{suffix}"));
    let (output, target) = target_add(fixture.root.path(), "claude", &target_path, "managed");
    assert!(output.status.success(), "target add failed: {target}");
    let target_id = target["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace = fixture.workspace.path().to_str().expect("workspace");
    let (output, binding) = binding_add(
        fixture.root.path(),
        "claude",
        "default",
        "exact-path",
        workspace,
        target_id,
    );
    assert!(output.status.success(), "binding add failed: {binding}");
    let binding_id = binding["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (output, projected) = skill_project(fixture.root.path(), "demo", binding_id, Some(method));
    assert!(output.status.success(), "projection failed: {projected}");
}

fn has_activation_gap(methods: &[&str]) -> bool {
    methods
        .windows(3)
        .any(|window| window == ["copy", "symlink", "copy"])
}

fn plan_with_activation_gap(fixture: &Fixture) -> Value {
    let mut planned = Value::Null;
    for index in 0..16 {
        let method = if index % 2 == 0 { "symlink" } else { "copy" };
        add_test_projection(fixture, &format!("{index}-{method}"), method);
        let (output, candidate) = plan_converge(fixture, &[]);
        assert!(output.status.success(), "plan failed: {candidate}");
        let has_gap = {
            let methods = candidate["data"]["effects"]
                .as_array()
                .expect("effects")
                .iter()
                .filter_map(|effect| effect["method"].as_str())
                .collect::<Vec<_>>();
            has_activation_gap(&methods)
        };
        planned = candidate;
        if has_gap {
            return planned;
        }
    }
    panic!("could not construct a noncontiguous projection activation plan: {planned}");
}

#[cfg(unix)]
#[test]
fn recovery_restores_noncontiguous_activated_projection_flags() {
    let fixture = projected_fixture();
    let planned = plan_with_activation_gap(&fixture);
    let methods = planned["data"]["effects"]
        .as_array()
        .expect("effects")
        .iter()
        .filter_map(|effect| effect["method"].as_str())
        .collect::<Vec<_>>();
    assert!(has_activation_gap(&methods));

    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "noncontiguous projection recovery\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "updated plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "noncontiguous-flags",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_all_projection_swaps",
        )],
    );
    assert!(
        !output.status.success(),
        "projection fault passed: {interrupted}"
    );
    let journal: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json"),
        )
        .expect("journal"),
    )
    .expect("parse journal");
    let flags = journal["projections"]
        .as_array()
        .expect("journal projections")
        .iter()
        .map(|projection| projection["activated"].as_bool().unwrap_or(false))
        .collect::<Vec<_>>();
    assert!(flags.windows(3).any(|window| window == [true, false, true]));
    assert_eq!(
        journal["installed_projections"],
        json!(flags.iter().filter(|flag| **flag).count())
    );

    let (output, interrupted_restore) = apply_plan(
        &fixture,
        &plan,
        "noncontiguous-flags",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_durable_projection_restore_intent",
        )],
    );
    assert!(
        !output.status.success(),
        "durable projection restore did not stop: {interrupted_restore}"
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "noncontiguous-flags", &[]);
    assert!(output.status.success(), "flag recovery failed: {recovered}");
    for effect in plan["data"]["effects"].as_array().expect("effects") {
        let live = std::path::Path::new(effect["materialized_path"].as_str().expect("path"));
        if effect["method"] == json!("symlink") {
            assert!(
                fs::symlink_metadata(live)
                    .expect("symlink projection")
                    .file_type()
                    .is_symlink()
            );
        } else {
            assert_eq!(
                fs::read_to_string(live.join("details.txt")).expect("copy projection"),
                "noncontiguous projection recovery\n"
            );
        }
    }
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    assert_exact_retained_ledger(&journal_path, "committed_artifacts_retained");
}

#[cfg(unix)]
#[test]
fn recovery_replays_rotation_and_post_exchange_activation_intent() {
    let fixture = projected_fixture();
    plan_with_activation_gap(&fixture);
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "activation intent recovery\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "updated plan failed: {plan}");
    let key = "rotation-activation-intent";

    let (output, source_stopped) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_commit",
        )],
    );
    assert!(
        !output.status.success(),
        "source fault passed: {source_stopped}"
    );
    let (output, rotation_stopped) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_projection_generation_rotation",
        )],
    );
    assert!(
        !output.status.success(),
        "rotation fault passed: {rotation_stopped}"
    );

    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let rotated_raw = fs::read(&journal_path).expect("journal");
    let rotated: Value = serde_json::from_slice(&rotated_raw).expect("parse rotated journal");
    assert_eq!(rotated["phase"], json!("preparing_projections"));
    assert!(
        rotated["projections"]
            .as_array()
            .expect("projections")
            .iter()
            .all(|projection| projection["activated_fingerprint"].is_null())
    );

    let methods = plan["data"]["effects"]
        .as_array()
        .expect("effects")
        .iter()
        .map(|effect| effect["method"].as_str().expect("method"))
        .collect::<Vec<_>>();
    let target = methods
        .windows(3)
        .position(|window| window == ["copy", "symlink", "copy"])
        .map(|index| index + 2)
        .expect("activation gap");
    let staging = rotated["projections"][target]["staging_path"]
        .as_str()
        .expect("target staging")
        .to_string();
    let backup = std::path::PathBuf::from(
        rotated["projections"][target]["backup"]["backup_path"]
            .as_str()
            .expect("target backup"),
    );
    let tamper = backup.join("concurrent-tamper.txt");
    let live_before = snapshot_tree(fixture.target.path());
    fs::write(&tamper, "tamper\n").expect("tamper rollback backup");
    let (output, rejected) = apply_plan(&fixture, &plan, key, &[]);
    assert!(
        !output.status.success(),
        "tampered backup resumed: {rejected}"
    );
    assert_eq!(rejected["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(snapshot_tree(fixture.target.path()), live_before);
    assert_eq!(
        fs::read(&journal_path).expect("preserved journal"),
        rotated_raw
    );
    fs::remove_file(tamper).expect("restore rollback backup");

    let (output, activation_stopped) = apply_plan(
        &fixture,
        &plan,
        key,
        &[
            (
                "LOOM_FAULT_INJECT",
                "convergence_interrupt_after_projection_activation",
            ),
            ("LOOM_TEST_CONVERGENCE_ACTIVATION_STAGING", &staging),
        ],
    );
    assert!(
        !output.status.success(),
        "post-exchange fault passed: {activation_stopped}"
    );
    let interrupted: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
        .expect("parse interrupted journal");
    let flags = interrupted["projections"]
        .as_array()
        .expect("projections")
        .iter()
        .map(|projection| projection["activated"].as_bool().unwrap_or(false))
        .collect::<Vec<_>>();
    let pending = interrupted["projections"]
        .as_array()
        .expect("projections")
        .iter()
        .map(|projection| projection["activation_pending"].as_bool().unwrap_or(false))
        .collect::<Vec<_>>();
    assert_eq!(&flags[target - 2..=target], &[true, false, false]);
    assert_eq!(&pending[target - 2..=target], &[false, false, true]);
    assert_eq!(
        interrupted["installed_projections"],
        json!(flags.iter().filter(|flag| **flag).count())
    );

    let (output, recovered) = apply_plan(&fixture, &plan, key, &[]);
    assert!(
        output.status.success(),
        "intent recovery failed: {recovered}"
    );
    assert_exact_retained_ledger(&journal_path, "committed_artifacts_retained");
}

#[cfg(unix)]
#[test]
fn restored_projection_concurrent_bytes_are_not_resealed() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "transaction projection bytes\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "concurrent-restored-projection";
    let (output, stopped) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_all_projection_swaps",
        )],
    );
    assert!(
        !output.status.success(),
        "projection swap fault passed: {stopped}"
    );
    let (output, restore_stopped) = apply_plan(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_durable_projection_restore_intent",
        )],
    );
    assert!(
        !output.status.success(),
        "projection restore fault passed: {restore_stopped}"
    );
    let live = fixture.target.path().join("demo/details.txt");
    fs::write(&live, "concurrent user bytes\n").expect("concurrent projection edit");

    let (output, rejected) = apply_plan(&fixture, &plan, key, &[]);
    assert!(
        !output.status.success(),
        "concurrent bytes were resealed: {rejected}"
    );
    assert_eq!(
        fs::read_to_string(&live).expect("preserved concurrent bytes"),
        "concurrent user bytes\n"
    );
}
