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
fn post_source_commit_fault_restores_all_surfaces_and_retries() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "post-source fault\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(fixture.target.path());
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));

    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "post-source-fault",
        &[("LOOM_FAULT_INJECT", "convergence_after_source_commit")],
    );
    assert!(
        !output.status.success(),
        "post-source fault passed: {failed}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    let journal = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
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
    assert_exact_retained_ledger(&journal, "rolled_back_artifacts_retained");

    let (output, recovered) = apply_plan(&fixture, &plan, "post-source-fault", &[]);
    assert!(
        output.status.success(),
        "post-source retry failed: {recovered}"
    );
    assert_eq!(recovered["data"]["complete"], json!(true));
    assert_eq!(
        git(
            fixture.root.path(),
            &[
                "rev-list",
                "--count",
                "--grep=skill(demo): converge source",
                "HEAD",
            ],
        )
        .trim(),
        "1"
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
    let retained = assert_exact_retained_ledger(&journal, "rolled_back_artifacts_retained");
    assert_eq!(retained["rollback_head"], json!(head.trim()));
    assert!(retained["rollback_index_digest"].as_str().is_some());

    let (output, recovered) = apply_plan(&fixture, &plan, "prepare-fault", &[]);
    assert!(
        output.status.success(),
        "preparation failure was not retryable: {recovered}"
    );
}

#[test]
fn partial_projection_preparation_retains_a_retryable_terminal_ledger() {
    let fixture = projected_fixture();
    add_copy_projection(&fixture, "partial-preparation");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "partial preparation\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        "partial-preparation",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_fail_after_first_projection_stage",
        )],
    );
    assert!(
        !output.status.success(),
        "preparation fault passed: {failed}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let mut journal = assert_exact_retained_ledger(&journal_path, "rolled_back_artifacts_retained");
    assert_eq!(journal["preparation_aborted"], json!(true));
    let fingerprints = journal["projections"]
        .as_array()
        .expect("projections")
        .iter()
        .filter(|projection| projection["activated_fingerprint"].is_string())
        .count();
    assert_eq!(fingerprints, 1);
    assert!(
        journal["ownership_attempts"]
            .as_array()
            .expect("attempts")
            .iter()
            .all(|attempt| matches!(attempt["state"].as_str(), Some("abandoned" | "retained")))
    );

    let retained = fs::read(&journal_path).expect("retained journal");
    let abandoned = journal["ownership_attempts"]
        .as_array_mut()
        .expect("attempts")
        .iter_mut()
        .find(|attempt| attempt["state"] == json!("abandoned"))
        .expect("abandoned attempt");
    abandoned["state"] = json!("ready");
    fs::write(
        &journal_path,
        serde_json::to_vec_pretty(&journal).expect("encode tampered journal"),
    )
    .expect("tamper journal");
    let (output, rejected) = apply_plan(&fixture, &plan, "partial-preparation", &[]);
    assert!(
        !output.status.success(),
        "nonterminal attempt was accepted: {rejected}"
    );
    fs::write(&journal_path, retained).expect("restore retained journal");

    let (output, recovered) = apply_plan(&fixture, &plan, "partial-preparation", &[]);
    assert!(
        output.status.success(),
        "partial preparation was not retryable: {recovered}"
    );
}
