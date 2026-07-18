use super::skill_convergence_executor::apply_plan;
use super::skill_convergence_ledger_assertions::{
    assert_exact_retained_ledger, snapshot_without_ledgered_paths,
    status_without_ledgered_transactions,
};
use super::*;

#[test]
fn local_faults_restore_all_surfaces() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "locally edited source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let status = git(fixture.root.path(), &["status", "--porcelain"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(fixture.target.path());
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "rollback",
        &[("LOOM_FAULT_INJECT", "convergence_after_registry_save")],
    );
    assert!(!output.status.success(), "fault did not fail: {failed}");
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    let journal = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    assert_eq!(
        status_without_ledgered_transactions(
            fixture.root.path(),
            &git(fixture.root.path(), &["status", "--porcelain"]),
            &journal,
            "rolled_back_artifacts_retained",
        ),
        status
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(
        snapshot_without_ledgered_paths(
            fixture.target.path(),
            &journal,
            "rolled_back_artifacts_retained",
        ),
        target
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
    );
}

#[test]
fn preparation_failure_retains_exact_artifact_ledger() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "prepared source\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let status = git(fixture.root.path(), &["status", "--porcelain"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(fixture.target.path());
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let backups = snapshot_tree(&fixture.root.path().join("state/backups"));
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "prepare-fault",
        &[("LOOM_FAULT_INJECT", "convergence_during_backup_preparation")],
    );
    assert!(
        !output.status.success(),
        "preparation fault passed: {failed}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    let journal = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    assert_eq!(
        status_without_ledgered_transactions(
            fixture.root.path(),
            &git(fixture.root.path(), &["status", "--porcelain"]),
            &journal,
            "rolled_back_artifacts_retained",
        ),
        status
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(
        snapshot_without_ledgered_paths(
            fixture.target.path(),
            &journal,
            "rolled_back_artifacts_retained",
        ),
        target
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/backups")),
        backups
    );
    assert_exact_retained_ledger(&journal, "rolled_back_artifacts_retained");
}
