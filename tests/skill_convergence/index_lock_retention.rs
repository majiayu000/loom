use super::*;
use crate::skill_convergence_executor::apply_plan;

#[test]
fn post_publication_failure_keeps_recovery_phase_and_replays_the_lock() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "retained source index lock\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");

    let key = "retained-source-index-lock";
    let (output, failed) = apply_plan(
        &fixture,
        &plan,
        key,
        &[("LOOM_TEST_PREPARED_INDEX_FAIL_AFTER_PUBLICATION", "1")],
    );
    assert!(!output.status.success(), "test failure was ignored: {failed}");
    assert_eq!(
        failed["error"]["details"]["index_lock_retained"],
        json!(true)
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("committing_source"));
    assert!(fixture.root.path().join(".git/index.lock").is_file());

    let (output, recovered) = apply_plan(&fixture, &plan, key, &[]);
    assert!(output.status.success(), "lock recovery failed: {recovered}");
    assert!(!fixture.root.path().join(".git/index.lock").exists());
}
