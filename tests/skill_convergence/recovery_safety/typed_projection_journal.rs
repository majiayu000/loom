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

struct RecoverySurfaceSnapshot {
    journal: Vec<u8>,
    artifacts: BTreeMap<String, Vec<u8>>,
    live: Vec<(PathBuf, BTreeMap<String, Vec<u8>>)>,
    source: BTreeMap<String, Vec<u8>>,
    registry: BTreeMap<String, Vec<u8>>,
    index: Vec<u8>,
    head: String,
}

fn recovery_surface_snapshot(
    fixture: &Fixture,
    journal_path: &Path,
    journal: &Value,
) -> RecoverySurfaceSnapshot {
    let artifact_root = PathBuf::from(journal["artifact_root"].as_str().expect("artifact root"));
    RecoverySurfaceSnapshot {
        journal: fs::read(journal_path).expect("journal bytes"),
        artifacts: snapshot_tree(&artifact_root),
        live: live_projection_snapshots(journal),
        source: snapshot_tree(&fixture.root.path().join("skills/demo")),
        registry: snapshot_tree(&fixture.root.path().join("state/registry")),
        index: fs::read(fixture.root.path().join(".git/index")).expect("index"),
        head: git(fixture.root.path(), &["rev-parse", "HEAD"]),
    }
}

fn assert_recovery_surfaces_unchanged(
    fixture: &Fixture,
    journal_path: &Path,
    journal: &Value,
    before: &RecoverySurfaceSnapshot,
) {
    let artifact_root = PathBuf::from(journal["artifact_root"].as_str().expect("artifact root"));
    assert_eq!(
        fs::read(journal_path).expect("journal after"),
        before.journal
    );
    assert_eq!(snapshot_tree(&artifact_root), before.artifacts);
    assert_live_projection_snapshots_unchanged(&before.live);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        before.source
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        before.registry
    );
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).expect("index after"),
        before.index
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        before.head
    );
}

fn claim_path(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().expect("claim base name").to_string_lossy();
    path.with_file_name(format!("{name}{suffix}"))
}

fn write_journal(path: &Path, journal: &Value) {
    fs::write(
        path,
        serde_json::to_vec_pretty(journal).expect("serialize journal"),
    )
    .expect("write journal");
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

#[test]
fn created_rollback_claim_live_collision_fails_closed_before_mutation() {
    let fixture = projected_fixture();
    let plan = create_projection_plan(&fixture);
    let key = "created-claim-live-collision";
    let (output, interrupted) = apply_with_faults(
        &fixture,
        &plan,
        key,
        &[
            ("LOOM_FAULT_INJECT", "convergence_after_registry_save"),
            (
                "LOOM_ROLLBACK_FAULT_INJECT",
                "convergence_interrupt_after_registry_restore",
            ),
        ],
    );
    assert!(
        !output.status.success(),
        "rollback boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    let original_journal = fs::read(&journal_path).expect("original journal");
    let live = PathBuf::from(
        journal["projections"][0]["materialized_path"]
            .as_str()
            .expect("live"),
    );
    let staging = PathBuf::from(
        journal["projections"][0]["staging_path"]
            .as_str()
            .expect("staging"),
    );
    let claim = claim_path(&staging, ".pending-cleanup-claim");
    let digest = journal["projections"][0]["rollback"]["activated_digest"]
        .as_str()
        .expect("activated digest")
        .to_string();
    fs::rename(&live, &claim).expect("simulate rollback cleanup claim syscall gap");
    journal["projections"][0]["rollback"] = json!({
        "kind": "pending_cleanup",
        "materialized_path": claim,
        "artifact_path": staging,
        "expected_live_digest": digest,
        "expected_digest": digest,
        "reason": "rollback_created",
    });
    journal["projections"][0]["state"] = json!("rollback_cleanup_pending");
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "claim/live collision resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::rename(&claim, &live).expect("repair syscall gap");
    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "collision repair retry failed: {recovered}"
    );
}

#[test]
fn external_sibling_rollback_evidence_fails_closed_before_mutation() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "external sibling\n",
    )
    .unwrap();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "external-sibling-evidence";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_committing_registry"),
    );
    assert!(
        !output.status.success(),
        "commit boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    let original_journal = fs::read(&journal_path).expect("original journal");
    let backup = rollback_artifact_path(&journal, 0);
    let external = backup.with_file_name("external-sibling-rollback-evidence");
    fs::rename(&backup, &external).expect("move rollback evidence to malicious sibling");
    journal["projections"][0]["rollback"]["backup_path"] = json!(external.display().to_string());
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "external sibling resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::rename(&external, &backup).expect("restore rollback evidence");
    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "external sibling repair failed: {recovered}"
    );
}

#[test]
fn prepared_cleanup_claim_collision_fails_closed_before_cleanup() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "prepared collision\n",
    )
    .unwrap();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "prepared-claim-collision";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_prepared"),
    );
    assert!(
        !output.status.success(),
        "prepared boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    let original_journal = fs::read(&journal_path).expect("original journal");
    let staging = PathBuf::from(
        journal["projections"][0]["staging_path"]
            .as_str()
            .expect("staging"),
    );
    journal["projections"][0]["prepared"]["source_path"] = json!(
        claim_path(&staging, ".prepared-cleanup-claim")
            .display()
            .to_string()
    );
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "prepared claim collision resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "prepared collision repair failed: {recovered}"
    );
}

#[test]
fn nested_prepared_projection_identity_tampering_is_zero_mutation() {
    for field in ["materialized_path", "instance_id"] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("nested prepared {field}\n"),
        )
        .unwrap();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("nested-prepared-{field}");
        let (output, interrupted) = apply(
            &fixture,
            &plan,
            &key,
            Some("convergence_interrupt_after_prepared"),
        );
        assert!(
            !output.status.success(),
            "prepared boundary passed: {interrupted}"
        );
        let (journal_path, mut journal) = transaction_journal(&fixture);
        let original_journal = fs::read(&journal_path).expect("original journal");
        journal["projections"][0]["prepared"]["projection"][field] = if field == "materialized_path"
        {
            json!(
                fixture
                    .target
                    .path()
                    .join("external-sibling")
                    .display()
                    .to_string()
            )
        } else {
            json!("malicious-instance")
        };
        write_journal(&journal_path, &journal);
        let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

        let (output, rejected) = apply(&fixture, &plan, &key, None);
        assert!(
            !output.status.success(),
            "nested {field} tampering resumed: {rejected}"
        );
        assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

        fs::write(&journal_path, original_journal).expect("restore journal");
        let (output, recovered) = apply(&fixture, &plan, &key, None);
        assert!(
            output.status.success(),
            "nested {field} repair failed: {recovered}"
        );
    }
}

#[test]
fn finalize_claim_live_collision_fails_closed_before_cleanup() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "finalize collision\n",
    )
    .unwrap();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "finalize-claim-collision";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_committing_registry"),
    );
    assert!(
        !output.status.success(),
        "commit boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    let original_journal = fs::read(&journal_path).expect("original journal");
    let staging = PathBuf::from(
        journal["projections"][0]["staging_path"]
            .as_str()
            .expect("staging"),
    );
    journal["projections"][0]["rollback"]["materialized_path"] = json!(
        claim_path(&staging, ".finalize-claim")
            .display()
            .to_string()
    );
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "finalize claim collision resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "finalize collision repair failed: {recovered}"
    );
}

#[test]
fn partial_rollback_late_owner_or_reservation_corruption_is_zero_mutation() {
    for mode in ["owner", "reservation"] {
        let fixture = projected_fixture();
        add_copy_projection(&fixture, &format!("{mode}-second"));
        add_copy_projection(&fixture, &format!("{mode}-third"));
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("{mode}\n"),
        )
        .unwrap();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("late-{mode}-corruption");
        let (output, interrupted) = apply_with_faults(
            &fixture,
            &plan,
            &key,
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
        let owner = PathBuf::from(
            journal["projections"][0]["staging_owner"]
                .as_str()
                .expect("owner"),
        );
        let attacked = if mode == "owner" {
            let path = owner.join(".owner");
            fs::write(&path, "external\n").expect("corrupt owner");
            path
        } else {
            let plan_id = journal["plan_id"].as_str().expect("plan id");
            let name = owner.file_name().expect("owner name").to_string_lossy();
            let path = owner.with_file_name(format!(".{name}.reservation-{plan_id}"));
            fs::write(&path, "external\n").expect("corrupt reservation");
            path
        };
        let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

        let (output, rejected) = apply(&fixture, &plan, &key, None);
        assert!(
            !output.status.success(),
            "late {mode} corruption resumed: {rejected}"
        );
        assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

        if mode == "owner" {
            fs::write(
                &attacked,
                format!("{}\n", journal["plan_id"].as_str().unwrap()),
            )
            .expect("repair owner");
        } else {
            fs::remove_file(&attacked).expect("repair reservation");
        }
        let (output, recovered) = apply(&fixture, &plan, &key, None);
        assert!(
            output.status.success(),
            "late {mode} repair failed: {recovered}"
        );
    }
}
