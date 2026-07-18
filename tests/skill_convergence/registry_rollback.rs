use super::*;
use common::run_loom_with_env;

#[test]
fn rolling_back_registry_restore_preserves_an_external_second_writer() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry rollback race\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    let (output, interrupted) = run_loom_with_env(
        fixture.root.path(),
        &[
            ("LOOM_FAULT_INJECT", "convergence_after_registry_save"),
            (
                "LOOM_ROLLBACK_FAULT_INJECT",
                "convergence_interrupt_after_registry_restore",
            ),
        ],
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "registry-rollback-cas",
        ],
    );
    assert!(
        !output.status.success(),
        "rollback interruption passed: {interrupted}"
    );

    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("rolling_back"));

    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let mut external: Value = serde_json::from_slice(
        &fs::read(&registry_path).expect("registry after interrupted rollback"),
    )
    .expect("parse registry");
    external["projections"][0]["observed_drift"] = json!(true);
    let external = serde_json::to_vec_pretty(&external).expect("encode external registry");
    fs::write(&registry_path, &external).expect("install external registry value");

    let (output, retry) = apply(&fixture, &plan, "registry-rollback-cas", None);
    assert!(
        !output.status.success(),
        "external registry was overwritten: {retry}"
    );
    assert_eq!(
        fs::read(&registry_path).expect("preserved registry"),
        external,
        "rollback overwrote the external second writer"
    );
    assert!(
        journal_path.is_file(),
        "failed recovery discarded its journal"
    );
}
