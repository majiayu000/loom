use std::fs;
use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::skill_convergence_executor::apply_plan;

fn seed_owned_index_lock(prepared: &Path, lock: &Path) {
    let mut claim_name = prepared.as_os_str().to_os_string();
    claim_name.push(".lock-claim");
    let claim = PathBuf::from(claim_name);
    let detached = prepared.with_extension("detached-index");
    fs::hard_link(prepared, &claim).expect("durable index claim");
    fs::copy(prepared, &detached).expect("copy detached evidence");
    fs::remove_file(prepared).expect("detach prepared name");
    fs::rename(&detached, prepared).expect("restore detached prepared evidence");
    fs::hard_link(&claim, lock).expect("publish owned index lock");
}

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
    assert!(journal["projections"][0]["backup"]["fingerprint"].is_string());

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
    seed_owned_index_lock(prepared, &fixture.root.path().join(".git/index.lock"));
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
fn source_recovery_adopts_only_its_durable_index_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "source lock crash\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "source-lock-crash",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_staged_index_prepared",
        )],
    );
    assert!(
        !output.status.success(),
        "source index preparation did not stop: {interrupted}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(journal_path).expect("journal")).expect("parse journal");
    let prepared =
        Path::new(journal["artifact_root"].as_str().expect("artifact root")).join("source-index");
    let lock = fixture.root.path().join(".git/index.lock");
    seed_owned_index_lock(&prepared, &lock);

    let (output, recovered) = apply_plan(&fixture, &plan, "source-lock-crash", &[]);
    assert!(
        output.status.success(),
        "owned source lock recovery failed: {recovered}"
    );
    assert!(!lock.exists());
}

#[test]
fn source_recovery_preserves_a_foreign_index_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "foreign source lock\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "foreign-source-lock",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_staged_index_prepared",
        )],
    );
    assert!(
        !output.status.success(),
        "source preparation passed: {interrupted}"
    );
    let lock = fixture.root.path().join(".git/index.lock");
    let foreign = b"foreign index lock\n";
    fs::write(&lock, foreign).expect("foreign lock");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let index = fs::read(fixture.root.path().join(".git/index")).expect("active index");

    let (output, rejected) = apply_plan(&fixture, &plan, "foreign-source-lock", &[]);
    assert!(
        !output.status.success(),
        "foreign lock was adopted: {rejected}"
    );
    assert_eq!(fs::read(&lock).expect("preserved lock"), foreign);
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).expect("index"),
        index
    );
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
    super::skill_convergence_executor::assert_exact_retained_ledger(
        &journal_path,
        "committed_artifacts_retained",
    );
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
            let journal_path = transaction_dir.join("convergence-demo.json");
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
            assert!(
                !journal_path.exists(),
                "stale active journal was not retired"
            );
            assert!(
                fs::read_dir(&transaction_dir)
                    .expect("transaction directory")
                    .filter_map(Result::ok)
                    .any(|entry| entry.file_name().to_string_lossy().starts_with("retained-")),
                "stale journal retained evidence was not archived"
            );
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

#[test]
fn partial_declared_backup_is_rebuilt_on_preparation_recovery() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "declared backup recovery\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, stopped) = apply_plan(
        &fixture,
        &plan,
        "partial-declared-backup",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_declared_backup",
        )],
    );
    assert!(!output.status.success(), "backup fault passed: {stopped}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("preparing"));
    let backup = std::path::PathBuf::from(
        journal["projections"][0]["backup"]["backup_path"]
            .as_str()
            .expect("projection backup"),
    );
    fs::remove_dir_all(&backup).expect("remove complete backup");
    fs::create_dir_all(&backup).expect("partial backup directory");
    fs::write(backup.join("partial.txt"), "partial\n").expect("partial backup");

    let (output, recovered) = apply_plan(&fixture, &plan, "partial-declared-backup", &[]);
    assert!(
        output.status.success(),
        "backup recovery failed: {recovered}"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/details.txt")).unwrap(),
        "declared backup recovery\n"
    );
}

#[test]
fn partial_index_backup_is_rebuilt_before_digest_persistence() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "index backup recovery\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, stopped) = apply_plan(
        &fixture,
        &plan,
        "partial-index-backup",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_index_snapshot",
        )],
    );
    assert!(
        !output.status.success(),
        "index snapshot fault passed: {stopped}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert!(journal["index_backup_digest"].is_null());
    let backup = Path::new(journal["index_backup"].as_str().expect("index backup"));
    fs::write(backup, b"partial index\n").expect("corrupt partial index backup");

    let (output, recovered) = apply_plan(&fixture, &plan, "partial-index-backup", &[]);
    assert!(
        output.status.success(),
        "index backup recovery failed: {recovered}"
    );
    assert!(!fixture.root.path().join(".git/index.lock").exists());
}

#[cfg(unix)]
#[test]
fn partial_symlink_backup_is_rebuilt_on_preparation_recovery() {
    let fixture = projected_fixture_with_method("symlink");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, stopped) = apply_plan(
        &fixture,
        &plan,
        "partial-symlink-backup",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_declared_backup",
        )],
    );
    assert!(!output.status.success(), "backup fault passed: {stopped}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    let backup = std::path::PathBuf::from(
        journal["projections"][0]["backup"]["backup_path"]
            .as_str()
            .expect("symlink backup"),
    );
    fs::remove_file(backup.join("symlink.json")).expect("make symlink backup partial");

    let (output, recovered) = apply_plan(&fixture, &plan, "partial-symlink-backup", &[]);
    assert!(
        output.status.success(),
        "symlink backup recovery failed: {recovered}"
    );
    assert!(
        fs::symlink_metadata(fixture.target.path().join("demo"))
            .expect("live projection")
            .file_type()
            .is_symlink()
    );
}
