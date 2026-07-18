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

#[cfg(unix)]
#[test]
fn create_plan_rejects_safe_symlink_created_after_review() {
    use std::os::unix::fs::symlink;

    let fixture = projected_fixture_with_method("symlink");
    fs::remove_file(fixture.target.path().join("demo")).expect("remove projection");
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
        &["commit", "-m", "test: prepare missing symlink"],
    );
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "create plan failed: {plan}");
    assert_eq!(plan["data"]["effects"][0]["effect"], json!("create"));

    symlink(
        fixture.root.path().join("skills/demo"),
        fixture.target.path().join("demo"),
    )
    .expect("late safe symlink");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, rejected) = apply_plan(&fixture, &plan, "late-safe-symlink", &[]);
    assert!(
        !output.status.success(),
        "late symlink was accepted: {rejected}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert!(
        !fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .exists()
    );
}

#[test]
fn registry_recovery_adopts_only_its_durable_index_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry lock crash\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "registry-lock-crash",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_before_registry_cas",
        )],
    );
    assert!(
        !output.status.success(),
        "registry CAS did not stop: {interrupted}"
    );

    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    let registry_commit = journal["registry_commit"]
        .as_str()
        .expect("registry commit");
    let source_head = journal["source_head"].as_str().expect("source head");
    let commit_attempt = journal["registry_index_attempts"]
        .as_array()
        .expect("registry index attempts")
        .iter()
        .rev()
        .find(|attempt| attempt["purpose"] == json!("commit"))
        .expect("registry commit index attempt");
    let prepared = Path::new(
        commit_attempt["prepared_index"]
            .as_str()
            .expect("prepared index"),
    );
    fs::copy(prepared, fixture.root.path().join(".git/index.lock"))
        .expect("simulate retained transaction lock");
    git(
        fixture.root.path(),
        &["update-ref", "HEAD", registry_commit, source_head],
    );

    let (output, recovered) = apply_plan(&fixture, &plan, "registry-lock-crash", &[]);
    assert!(
        output.status.success(),
        "owned lock recovery failed: {recovered}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]).trim(),
        registry_commit
    );
    assert!(!fixture.root.path().join(".git/index.lock").exists());
}

#[test]
fn source_cas_does_not_adopt_a_following_external_head() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "source CAS race\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "source-cas-race",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_cas",
        )],
    );
    assert!(
        !output.status.success(),
        "source CAS did not stop: {interrupted}"
    );
    fs::write(fixture.root.path().join("external.txt"), "external\n").expect("external file");
    git(fixture.root.path(), &["add", "external.txt"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: external after source CAS"],
    );
    let external_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, rejected) = apply_plan(&fixture, &plan, "source-cas-race", &[]);
    assert!(
        !output.status.success(),
        "external source HEAD accepted: {rejected}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        external_head
    );
}

#[test]
fn foreign_index_lock_with_interrupted_rollback_remains_retryable() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let lock = fixture.root.path().join(".git/index.lock");
    let foreign = b"foreign Git process lock\n";
    fs::write(&lock, foreign).expect("foreign index lock");

    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "foreign-lock-rollback",
        &[(
            "LOOM_ROLLBACK_FAULT_INJECT",
            "convergence_interrupt_after_registry_restore",
        )],
    );
    assert!(
        !output.status.success(),
        "foreign lock unexpectedly applied: {interrupted}"
    );
    assert_eq!(fs::read(&lock).expect("preserved foreign lock"), foreign);
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("rolling_back"));
    assert!(journal["registry_commit"].is_null());
    assert!(journal["registry_staged_index_digest"].is_null());

    fs::remove_file(&lock).expect("release foreign lock");
    let (output, recovered) = apply_plan(&fixture, &plan, "foreign-lock-rollback", &[]);
    assert!(
        output.status.success(),
        "rollback retry failed: {recovered}"
    );
    super::skill_convergence_executor::assert_exact_retained_ledger(
        &journal_path,
        "committed_artifacts_retained",
    );
}

#[test]
fn committed_cleanup_accepts_an_unrelated_descendant_commit() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "cleanup descendant\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "cleanup-descendant",
        &[("LOOM_FAULT_INJECT", "convergence_interrupt_during_cleanup")],
    );
    assert!(
        !output.status.success(),
        "cleanup fault passed: {interrupted}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    assert!(
        journal_path.is_file(),
        "cleanup journal must remain durable"
    );

    fs::write(fixture.root.path().join("external.txt"), "external\n").expect("external file");
    git(fixture.root.path(), &["add", "external.txt"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: descendant after convergence"],
    );
    let descendant = git(fixture.root.path(), &["rev-parse", "HEAD"]);

    let (output, recovered) = apply_plan(&fixture, &plan, "cleanup-descendant", &[]);
    assert!(
        output.status.success(),
        "descendant blocked cleanup-only recovery: {recovered}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), descendant);
    assert!(!journal_path.exists());
}

#[test]
fn source_committed_recovery_rechecks_checkpoint_evidence() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "checkpoint recovery\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "checkpoint-recovery",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_commit",
        )],
    );
    assert!(
        !output.status.success(),
        "source fault passed: {interrupted}"
    );
    let target_before = snapshot_tree(fixture.target.path());

    let checkpoint_path = fixture
        .root
        .path()
        .join("state/registry/ops/checkpoint.json");
    let mut checkpoint: Value =
        serde_json::from_slice(&fs::read(&checkpoint_path).expect("checkpoint"))
            .expect("parse checkpoint");
    checkpoint["updated_at"] = json!("2000-01-01T00:00:00Z");
    fs::write(
        &checkpoint_path,
        serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint"),
    )
    .expect("drift checkpoint");

    let (output, rejected) = apply_plan(&fixture, &plan, "checkpoint-recovery", &[]);
    assert!(
        !output.status.success(),
        "checkpoint drift resumed recovery: {rejected}"
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
}

#[test]
fn pre_mutation_recovery_revalidates_all_routing_and_live_guards() {
    #[derive(Clone, Copy, Debug)]
    enum Drift {
        Checkpoint,
        Projections,
        LiveProjection,
    }

    let phases = [
        (
            "preparing",
            "convergence_interrupt_after_index_snapshot_digest",
        ),
        ("prepared", "convergence_interrupt_after_prepared"),
    ];
    let drifts = [Drift::Checkpoint, Drift::Projections, Drift::LiveProjection];

    for (phase, fault) in phases {
        for drift in drifts {
            let fixture = projected_fixture();
            fs::write(
                fixture.root.path().join("skills/demo/details.txt"),
                "recovery guard source\n",
            )
            .expect("edit source");
            let (output, plan) = plan_converge(&fixture, &[]);
            assert!(output.status.success(), "plan failed: {plan}");
            let key = format!("pre-mutation-{phase}-{drift:?}");
            let (output, interrupted) =
                apply_plan(&fixture, &plan, &key, &[("LOOM_FAULT_INJECT", fault)]);
            assert!(
                !output.status.success(),
                "{phase} fault did not interrupt: {interrupted}"
            );

            match drift {
                Drift::Checkpoint => {
                    let path = fixture
                        .root
                        .path()
                        .join("state/registry/ops/checkpoint.json");
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&path).expect("read checkpoint"))
                            .expect("parse checkpoint");
                    value["updated_at"] = json!("2000-01-01T00:00:00Z");
                    fs::write(
                        &path,
                        serde_json::to_vec_pretty(&value).expect("encode checkpoint"),
                    )
                    .expect("drift checkpoint");
                }
                Drift::Projections => {
                    let path = fixture.root.path().join("state/registry/projections.json");
                    let mut value: Value =
                        serde_json::from_slice(&fs::read(&path).expect("read projections"))
                            .expect("parse projections");
                    value["projections"][0]["observed_drift"] = json!(true);
                    fs::write(
                        &path,
                        serde_json::to_vec_pretty(&value).expect("encode projections"),
                    )
                    .expect("drift projections");
                }
                Drift::LiveProjection => {
                    fs::write(
                        fixture.target.path().join("demo/SKILL.md"),
                        "external live projection drift\n",
                    )
                    .expect("drift live projection");
                }
            }

            let transaction_dir = fixture.root.path().join("state/transactions");
            let transactions_before = snapshot_tree(&transaction_dir);
            let source_before = snapshot_tree(&fixture.root.path().join("skills/demo"));
            let target_before = snapshot_tree(fixture.target.path());
            let head_before = git(fixture.root.path(), &["rev-parse", "HEAD"]);
            let status_before = git(fixture.root.path(), &["status", "--porcelain"]);
            let index_before =
                fs::read(fixture.root.path().join(".git/index")).expect("read index");

            let (output, rejected) = apply_plan(&fixture, &plan, &key, &[]);
            assert!(
                !output.status.success(),
                "{phase} recovery accepted {drift:?}: {rejected}"
            );
            assert_eq!(snapshot_tree(&transaction_dir), transactions_before);
            assert_eq!(
                snapshot_tree(&fixture.root.path().join("skills/demo")),
                source_before
            );
            assert_eq!(snapshot_tree(fixture.target.path()), target_before);
            assert_eq!(
                git(fixture.root.path(), &["rev-parse", "HEAD"]),
                head_before
            );
            assert_eq!(
                git(fixture.root.path(), &["status", "--porcelain"]),
                status_before
            );
            assert_eq!(
                fs::read(fixture.root.path().join(".git/index")).expect("read index"),
                index_before
            );
        }
    }
}
