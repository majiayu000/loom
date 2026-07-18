use super::*;
use common::run_loom_with_env;

fn retry_apply(fixture: &Fixture, plan: &Value, key: &str) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    run_loom_with_env(
        fixture.root.path(),
        &[],
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

#[test]
fn retry_retains_superseded_registry_index_generation() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry index generation race\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    let (output, interrupted) = run_loom_with_env(
        fixture.root.path(),
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_before_registry_cas",
        )],
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "registry-index-generation",
        ],
    );
    assert!(
        !output.status.success(),
        "registry CAS interruption passed: {interrupted}"
    );

    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    let first_attempt = journal["registry_index_attempts"]
        .as_array()
        .expect("registry index attempts")
        .iter()
        .find(|attempt| attempt["purpose"] == json!("commit"))
        .expect("registry commit index attempt");
    let generation = first_attempt["generation"]
        .as_str()
        .expect("registry index generation")
        .to_string();
    let superseded = Path::new(
        first_attempt["base_index"]
            .as_str()
            .expect("base index path"),
    )
    .to_path_buf();
    assert!(first_attempt["base_digest"].as_str().is_some());
    assert!(first_attempt["prepared_digest"].as_str().is_some());
    assert!(first_attempt["commit_digest"].as_str().is_some());
    assert!(
        Path::new(
            first_attempt["commit_index"]
                .as_str()
                .expect("commit index path")
        )
        .is_file(),
        "commit index evidence was deleted"
    );
    assert!(superseded.is_file(), "first generation index is absent");
    let foreign = b"foreign replacement must survive";
    fs::write(&superseded, foreign).expect("replace superseded index");

    let (output, recovered) = retry_apply(&fixture, &plan, "registry-index-generation");
    assert!(output.status.success(), "recovery failed: {recovered}");
    assert_eq!(
        fs::read(&superseded).expect("read superseded index"),
        foreign,
        "recovery deleted or overwrote a superseded generation"
    );
    let recovered_journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("recovered journal"))
            .expect("parse recovered journal");
    let attempts = recovered_journal["registry_index_attempts"]
        .as_array()
        .expect("recovered registry index attempts");
    assert!(attempts.len() >= 2, "retry did not append a new generation");
    let superseded_attempt = attempts
        .iter()
        .find(|attempt| attempt["generation"] == json!(generation))
        .expect("superseded generation evidence");
    assert_eq!(superseded_attempt["state"], json!("abandoned"));
    assert_eq!(
        superseded_attempt["base_index"],
        json!(superseded.display().to_string())
    );
}

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

    let (output, retry) = retry_apply(&fixture, &plan, "registry-rollback-cas");
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
