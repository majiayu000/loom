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

#[cfg(unix)]
#[test]
fn recovery_restores_noncontiguous_activated_projection_flags() {
    let fixture = projected_fixture();
    let mut planned = Value::Null;
    for index in 0..16 {
        let method = if index % 2 == 0 { "symlink" } else { "copy" };
        add_test_projection(&fixture, &format!("{index}-{method}"), method);
        let (output, candidate) = plan_converge(&fixture, &[]);
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
            break;
        }
    }
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
