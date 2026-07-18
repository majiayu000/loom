use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use super::*;

fn apply_with_faults(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
    faults: &[(&str, &str)],
) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    run_loom_with_env(
        fixture.root.path(),
        faults,
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            key,
        ],
    )
}

fn transaction_journal(fixture: &Fixture) -> (PathBuf, Value) {
    let path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal =
        serde_json::from_slice(&fs::read(&path).expect("journal")).expect("parse journal");
    (path, journal)
}

fn rollback_artifact_path(journal: &Value, index: usize) -> PathBuf {
    let rollback = &journal["projections"][index]["rollback"];
    for field in ["backup_path", "rollback_path", "artifact_path"] {
        if let Some(path) = rollback[field].as_str() {
            return PathBuf::from(path);
        }
    }
    panic!("projection {index} has no rollback artifact path: {rollback}");
}

fn live_projection_snapshots(journal: &Value) -> Vec<(PathBuf, BTreeMap<String, Vec<u8>>)> {
    journal["projections"]
        .as_array()
        .expect("projections")
        .iter()
        .map(|projection| {
            let path = PathBuf::from(
                projection["materialized_path"]
                    .as_str()
                    .expect("materialized path"),
            );
            let snapshot = snapshot_tree(&path);
            (path, snapshot)
        })
        .collect()
}

fn assert_live_projection_snapshots_unchanged(before: &[(PathBuf, BTreeMap<String, Vec<u8>>)]) {
    for (path, snapshot) in before {
        assert_eq!(
            &snapshot_tree(path),
            snapshot,
            "projection changed: {path:?}"
        );
    }
}

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

#[test]
fn late_finalize_artifact_corruption_is_zero_mutation_before_batch_finalize() {
    let fixture = projected_fixture();
    add_copy_projection(&fixture, "finalize-second");
    add_copy_projection(&fixture, "finalize-third");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "finalize batch validation\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "late-finalize-artifact";
    let (output, interrupted) = apply_with_faults(
        &fixture,
        &plan,
        key,
        &[(
            "LOOM_CLEANUP_FAULT_INJECT",
            "convergence_interrupt_after_first_projection_finalize",
        )],
    );
    assert!(
        !output.status.success(),
        "finalize boundary passed: {interrupted}"
    );
    let (journal_path, journal) = transaction_journal(&fixture);
    assert_eq!(journal["phase"], "committed_cleanup_pending");
    assert_eq!(journal["projections"][0]["state"], "finalized");
    assert_eq!(journal["projections"][1]["state"], "activated");
    assert_eq!(journal["projections"][2]["state"], "activated");
    let artifact = rollback_artifact_path(&journal, 2);
    let corruption = artifact.join("external-corruption");
    fs::write(&corruption, "external\n").expect("corrupt late rollback artifact");
    let journal_before = fs::read(&journal_path).expect("journal bytes");
    let artifact_root = PathBuf::from(journal["artifact_root"].as_str().expect("artifact root"));
    let artifacts_before = snapshot_tree(&artifact_root);
    let live_before = live_projection_snapshots(&journal);
    let registry_before = snapshot_tree(&fixture.root.path().join("state/registry"));
    let head_before = git(fixture.root.path(), &["rev-parse", "HEAD"]);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "corrupt finalize resumed: {rejected}"
    );
    assert_eq!(
        fs::read(&journal_path).expect("journal after"),
        journal_before
    );
    assert_eq!(snapshot_tree(&artifact_root), artifacts_before);
    assert_live_projection_snapshots_unchanged(&live_before);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry_before
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_before
    );

    fs::remove_file(corruption).expect("repair rollback artifact");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "finalize retry failed: {recovered}"
    );
    assert!(!journal_path.exists());
}

#[test]
fn late_rollback_artifact_corruption_is_zero_mutation_before_batch_rollback() {
    let fixture = projected_fixture();
    add_copy_projection(&fixture, "rollback-second");
    add_copy_projection(&fixture, "rollback-third");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "rollback batch validation\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "late-rollback-artifact";
    let (output, interrupted) = apply_with_faults(
        &fixture,
        &plan,
        key,
        &[
            ("LOOM_FAULT_INJECT", "convergence_after_registry_save"),
            (
                "LOOM_ROLLBACK_FAULT_INJECT",
                "convergence_interrupt_after_first_projection_rollback",
            ),
        ],
    );
    assert!(
        !output.status.success(),
        "rollback boundary passed: {interrupted}"
    );
    let (journal_path, journal) = transaction_journal(&fixture);
    assert_eq!(journal["phase"], "rolling_back");
    assert_eq!(journal["projections"][0]["state"], "activated");
    assert_eq!(journal["projections"][1]["state"], "activated");
    assert_eq!(journal["projections"][2]["state"], "rolled_back");
    let artifact = rollback_artifact_path(&journal, 0);
    let corruption = artifact.join("external-corruption");
    fs::write(&corruption, "external\n").expect("corrupt late rollback artifact");
    let journal_before = fs::read(&journal_path).expect("journal bytes");
    let artifact_root = PathBuf::from(journal["artifact_root"].as_str().expect("artifact root"));
    let artifacts_before = snapshot_tree(&artifact_root);
    let live_before = live_projection_snapshots(&journal);
    let registry_before = snapshot_tree(&fixture.root.path().join("state/registry"));
    let head_before = git(fixture.root.path(), &["rev-parse", "HEAD"]);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "corrupt rollback resumed: {rejected}"
    );
    assert_eq!(
        fs::read(&journal_path).expect("journal after"),
        journal_before
    );
    assert_eq!(snapshot_tree(&artifact_root), artifacts_before);
    assert_live_projection_snapshots_unchanged(&live_before);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry_before
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_before
    );

    fs::remove_file(corruption).expect("repair rollback artifact");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "rollback retry failed: {recovered}"
    );
    assert!(!journal_path.exists());
}
