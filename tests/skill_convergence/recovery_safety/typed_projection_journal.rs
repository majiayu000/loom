use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use super::*;

#[test]
fn activation_syscall_before_journal_save_recovers_existing_and_created_paths() {
    for created in [false, true] {
        let fixture = projected_fixture();
        let plan = if created {
            create_projection_plan(&fixture)
        } else {
            fs::write(
                fixture.root.path().join("skills/demo/details.txt"),
                "activation gap source\n",
            )
            .expect("edit source");
            let (output, plan) = plan_converge(&fixture, &[]);
            assert!(output.status.success(), "plan failed: {plan}");
            plan
        };
        let key = if created {
            "activation-gap-created"
        } else {
            "activation-gap-existing"
        };
        let (output, interrupted) = apply(
            &fixture,
            &plan,
            key,
            Some("convergence_interrupt_after_projection_activation"),
        );
        assert!(
            !output.status.success(),
            "activation gap passed: {interrupted}"
        );
        let journal_path = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        let journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
            .expect("parse journal");
        assert_eq!(journal["phase"], "installing_projections");
        assert_eq!(journal["installed_projections"], 0);
        assert_eq!(journal["projections"][0]["state"], "prepared");
        let materialized = PathBuf::from(
            journal["projections"][0]["materialized_path"]
                .as_str()
                .expect("materialized path"),
        );
        let staging = PathBuf::from(
            journal["projections"][0]["prepared"]["staging_path"]
                .as_str()
                .expect("staging path"),
        );
        assert!(fs::symlink_metadata(&materialized).is_ok());
        if journal["projections"][0]["prepared"]["path_exists"] == true {
            assert_ne!(snapshot_tree(&materialized), snapshot_tree(&staging));
        } else {
            assert!(fs::symlink_metadata(&staging).is_err());
        }

        let (output, recovered) = apply(&fixture, &plan, key, None);
        assert!(
            output.status.success(),
            "activation recovery failed: {recovered}"
        );
        assert!(!journal_path.exists());
    }
}

#[test]
fn late_typed_projection_corruption_is_zero_mutation_across_all_projections() {
    let fixture = projected_fixture();
    add_copy_projection(&fixture, "typed-second");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "multi projection evidence\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "multi-typed-corruption";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_source_commit"),
    );
    assert!(!output.status.success(), "source gap passed: {interrupted}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    let late = journal["projections"]
        .as_array()
        .expect("projections")
        .last()
        .expect("late");
    let staging = PathBuf::from(late["prepared"]["staging_path"].as_str().expect("staging"));
    let corruption = staging.join("external-corruption");
    fs::write(&corruption, "external\n").expect("corrupt late prepared artifact");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(fixture.target.path());
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let index_path = PathBuf::from(journal["index_backup"].as_str().expect("index"));
    let index = fs::read(&index_path).expect("index bytes");

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "corrupt late artifact resumed: {rejected}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
    );
    assert_eq!(fs::read(&index_path).expect("index after"), index);
    assert!(journal_path.is_file());

    fs::remove_file(corruption).expect("restore prepared artifact");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "typed artifact retry failed: {recovered}"
    );
}
