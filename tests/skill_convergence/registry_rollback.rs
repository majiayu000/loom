use super::*;
use crate::skill_convergence_executor::apply_plan;
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
fn ready_registry_index_replacement_fails_closed_without_deletion() {
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
    assert!(
        !output.status.success(),
        "replacement was accepted by recovery: {recovered}"
    );
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
    assert_eq!(attempts.len(), 1, "failed validation mutated the ledger");
    let superseded_attempt = attempts
        .iter()
        .find(|attempt| attempt["generation"] == json!(generation))
        .expect("superseded generation evidence");
    assert_eq!(superseded_attempt["state"], json!("ready"));
    assert_eq!(
        superseded_attempt["base_index"],
        json!(superseded.display().to_string())
    );
}

#[test]
fn retry_appends_and_terminalizes_registry_index_generations() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry index generation retry\n",
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
            "registry-index-append",
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
    let interrupted_journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    let generation = interrupted_journal["registry_index_attempts"][0]["generation"]
        .as_str()
        .expect("first generation")
        .to_string();

    let (output, recovered) = retry_apply(&fixture, &plan, "registry-index-append");
    assert!(output.status.success(), "recovery failed: {recovered}");
    let recovered_journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("recovered journal"))
            .expect("parse recovered journal");
    let attempts = recovered_journal["registry_index_attempts"]
        .as_array()
        .expect("recovered registry index attempts");
    assert!(attempts.len() >= 2, "retry did not append a new generation");
    assert_eq!(
        attempts
            .iter()
            .find(|attempt| attempt["generation"] == json!(generation))
            .expect("superseded generation")["state"],
        json!("abandoned")
    );
    for attempt in attempts {
        assert!(matches!(
            attempt["state"].as_str(),
            Some("abandoned" | "retained")
        ));
        if attempt["state"] == json!("retained") {
            assert!(attempt["base_digest"].as_str().is_some());
            assert!(attempt["prepared_digest"].as_str().is_some());
            for (path, digest) in [
                ("base_index", "base_digest"),
                ("prepared_index", "prepared_digest"),
            ] {
                assert!(Path::new(attempt[path].as_str().expect(path)).is_file());
                assert!(attempt[digest].as_str().is_some());
            }
        }
    }
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

fn assert_external_registry_retired(
    fixture: &Fixture,
    plan: &Value,
    journal_path: &Path,
    registry_path: &Path,
    original_registry: &[u8],
    original_projection: &BTreeMap<String, Vec<u8>>,
    external_head: &str,
    expect_registry_attempt: bool,
) {
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        external_head
    );
    assert_eq!(
        fs::read(registry_path).expect("restored registry"),
        original_registry
    );
    assert_eq!(
        snapshot_tree(&fixture.target.path().join("demo")),
        *original_projection
    );
    assert!(!journal_path.exists(), "active journal was not retired");
    assert!(
        Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(fixture.root.path())
            .status()
            .expect("inspect external index")
            .success(),
        "transaction changed the external commit index"
    );
    let prefix = format!(
        "retained-{}-",
        plan["data"]["plan_id"].as_str().expect("plan id")
    );
    let retained = fs::read_dir(journal_path.parent().expect("journal parent"))
        .expect("retained journal directory")
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .expect("retained registry journal");
    let retained: Value =
        serde_json::from_slice(&fs::read(retained.path()).expect("retained journal"))
            .expect("parse retained journal");
    assert_eq!(retained["phase"], json!("rolled_back_artifacts_retained"));
    assert!(retained["registry_commit"].is_null());
    assert!(retained["registry_staged_index_digest"].is_null());
    assert_eq!(retained["rollback_head"], json!(external_head.trim()));
    assert!(retained["rollback_index_digest"].as_str().is_some());
    let attempts = retained["registry_index_attempts"]
        .as_array()
        .expect("registry index attempts");
    if expect_registry_attempt {
        assert!(!attempts.is_empty(), "registry attempt evidence is absent");
    }
    assert!(
        attempts
            .iter()
            .all(|attempt| matches!(attempt["state"].as_str(), Some("retained" | "abandoned")))
    );
}

#[cfg(unix)]
#[test]
fn external_head_after_registry_commit_preparation_clears_orphan_evidence() {
    let fixture = projected_fixture();
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let original_registry = fs::read(&registry_path).expect("original registry");
    let original_projection = snapshot_tree(&fixture.target.path().join("demo"));
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "late registry CAS race\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");

    let (output, rejected, external_head) = std::thread::scope(|scope| {
        let apply = scope.spawn(|| {
            apply_plan(
                &fixture,
                &plan,
                "late-registry-cas-race",
                &[("LOOM_TEST_CONVERGENCE_REGISTRY_CAS_PAUSE_MS", "2000")],
            )
        });
        let mut prepared = false;
        for _ in 0..200 {
            if let Ok(raw) = fs::read(&journal_path)
                && let Ok(journal) = serde_json::from_slice::<Value>(&raw)
                && journal["phase"] == json!("committing_registry")
                && !journal["registry_commit"].is_null()
                && !journal["registry_staged_index_digest"].is_null()
            {
                prepared = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(prepared, "registry commit evidence was never persisted");
        fs::write(fixture.root.path().join("external-head.txt"), "external\n")
            .expect("external file");
        git(fixture.root.path(), &["add", "external-head.txt"]);
        git(
            fixture.root.path(),
            &["commit", "-m", "test: late registry CAS head"],
        );
        let external_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let (output, rejected) = apply.join().expect("apply thread");
        (output, rejected, external_head)
    });
    assert!(
        !output.status.success(),
        "external HEAD was accepted: {rejected}"
    );
    assert_external_registry_retired(
        &fixture,
        &plan,
        &journal_path,
        &registry_path,
        &original_registry,
        &original_projection,
        &external_head,
        true,
    );
}

#[cfg(unix)]
#[test]
fn registry_json_cas_interruption_recovers_after_external_head() {
    let fixture = projected_fixture();
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let original_registry = fs::read(&registry_path).expect("original registry");
    let original_projection = snapshot_tree(&fixture.target.path().join("demo"));
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry JSON CAS interruption\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "registry-json-cas-crash",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_registry_save_cas",
        )],
    );
    assert!(
        !output.status.success(),
        "CAS interruption passed: {interrupted}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("projections_swapped"));
    assert_ne!(
        fs::read(&registry_path).expect("mutated registry"),
        original_registry
    );

    fs::write(fixture.root.path().join("external-head.txt"), "external\n").expect("external file");
    git(fixture.root.path(), &["add", "external-head.txt"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: post-JSON-CAS head"],
    );
    let external_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, rejected) = retry_apply(&fixture, &plan, "registry-json-cas-crash");
    assert!(
        !output.status.success(),
        "stale plan was accepted after recovery: {rejected}"
    );
    assert_external_registry_retired(
        &fixture,
        &plan,
        &journal_path,
        &registry_path,
        &original_registry,
        &original_projection,
        &external_head,
        false,
    );
    let (output, fresh) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "fresh plan failed: {fresh}");
    let (output, applied) = apply_plan(&fixture, &fresh, "post-cas-fresh", &[]);
    assert!(output.status.success(), "fresh apply failed: {applied}");
}
