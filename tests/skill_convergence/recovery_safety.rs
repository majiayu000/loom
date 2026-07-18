use std::fs;

use common::run_loom_with_env;

use super::*;

fn apply(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
    fault: Option<&str>,
) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    let env = fault
        .map(|value| vec![("LOOM_FAULT_INJECT", value)])
        .unwrap_or_default();
    run_loom_with_env(
        fixture.root.path(),
        &env,
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
fn internal_reservation_staging_collisions_are_preserved() {
    for kind in ["artifact", "projection"] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
        let collision = if kind == "artifact" {
            fixture.root.path().join(format!(
                "state/transactions/.{plan_id}-artifacts.staging-{plan_id}"
            ))
        } else {
            fixture.target.path().join(format!(
                "..loom-projection-stage-{plan_id}-0.owner.staging-{plan_id}"
            ))
        };
        fs::create_dir_all(&collision).expect("collision dir");
        fs::write(collision.join("keep"), format!("unowned-{kind}\n")).expect("collision file");
        let (output, rejected) = apply(&fixture, &plan, kind, None);
        assert!(!output.status.success(), "collision applied: {rejected}");
        assert_eq!(
            fs::read_to_string(collision.join("keep")).expect("preserved collision"),
            format!("unowned-{kind}\n")
        );
    }
}

#[test]
fn refresh_journal_rejects_null_backup_before_cleanup() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let fault = "convergence_interrupt_after_owner_marker_write";
    let (output, interrupted) = apply(&fixture, &plan, "null-backup", Some(fault));
    assert!(!output.status.success(), "fault passed: {interrupted}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let mut journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    journal["projections"][0]["backup"] = Value::Null;
    fs::write(
        &journal_path,
        serde_json::to_vec_pretty(&journal).expect("encode"),
    )
    .expect("journal write");
    let transaction_paths = super::skill_convergence_executor::all_paths(
        &fixture.root.path().join("state/transactions"),
    );
    let (output, rejected) = apply(&fixture, &plan, "null-backup", None);
    assert!(
        !output.status.success(),
        "null backup recovered: {rejected}"
    );
    assert_eq!(
        super::skill_convergence_executor::all_paths(
            &fixture.root.path().join("state/transactions"),
        ),
        transaction_paths,
        "invalid journal triggered cleanup"
    );
}

#[test]
fn prepared_recovery_preserves_external_target_creation() {
    let fixture = projected_fixture();
    fs::remove_dir_all(fixture.target.path().join("demo")).expect("remove target");
    let projections_path = fixture.root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_slice(&fs::read(&projections_path).expect("registry"))
            .expect("parse registry");
    projections["projections"] = json!([]);
    fs::write(
        &projections_path,
        serde_json::to_vec_pretty(&projections).expect("encode"),
    )
    .expect("registry write");
    git(
        fixture.root.path(),
        &["add", "state/registry/projections.json"],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: create projection plan"],
    );
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let fault = "convergence_interrupt_after_prepared";
    let (output, interrupted) = apply(&fixture, &plan, "prepared", Some(fault));
    assert!(!output.status.success(), "fault passed: {interrupted}");
    fs::create_dir(fixture.target.path().join("demo")).expect("external target");
    fs::write(fixture.target.path().join("demo/external"), "external\n").expect("external file");
    let (output, rejected) = apply(&fixture, &plan, "prepared", None);
    assert!(!output.status.success(), "drifted plan applied: {rejected}");
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/external"))
            .expect("external preserved"),
        "external\n"
    );
}

#[test]
fn original_registry_snapshot_is_rederived_before_cleanup() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let fault = "convergence_interrupt_after_prepared";
    let (output, interrupted) = apply(&fixture, &plan, "original-registry", Some(fault));
    assert!(!output.status.success(), "fault passed: {interrupted}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let mut journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    journal["original_projections"]["projections"] = json!([]);
    fs::write(
        &journal_path,
        serde_json::to_vec_pretty(&journal).expect("encode"),
    )
    .expect("write journal");
    let before = super::skill_convergence_executor::all_paths(fixture.root.path());
    let (output, rejected) = apply(&fixture, &plan, "original-registry", None);
    assert!(
        !output.status.success(),
        "forged registry snapshot recovered: {rejected}"
    );
    assert_eq!(
        super::skill_convergence_executor::all_paths(fixture.root.path()),
        before
    );
}

#[test]
fn registry_commit_scope_preserves_unrelated_dirty_state() {
    for fault in [None, Some("convergence_interrupt_committing_registry")] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            "scoped registry source\n",
        )
        .expect("source edit");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let original_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let gitignore = fixture.root.path().join(".gitignore");
        let mut ignore = fs::read_to_string(&gitignore).unwrap_or_default();
        ignore.push_str("unrelated-ignore-entry\n");
        fs::write(&gitignore, ignore).expect("dirty gitignore");
        fs::write(
            fixture.root.path().join("state/registry/unrelated.json"),
            "{\"unrelated\":true}\n",
        )
        .expect("dirty registry");
        fs::create_dir_all(fixture.root.path().join("state/v3")).expect("v3 dir");
        fs::write(
            fixture.root.path().join("state/v3/unrelated"),
            "unrelated\n",
        )
        .expect("dirty v3");
        git(
            fixture.root.path(),
            &["add", ".gitignore", "state/v3/unrelated"],
        );

        let (first_output, first) = apply(&fixture, &plan, "scoped-registry", fault);
        let result = if fault.is_some() {
            assert!(!first_output.status.success(), "fault passed: {first}");
            let (output, body) = apply(&fixture, &plan, "scoped-registry", None);
            assert!(output.status.success(), "recovery failed: {body}");
            body
        } else {
            assert!(first_output.status.success(), "apply failed: {first}");
            first
        };
        assert!(result["data"]["applied"].is_object());
        let committed = git(
            fixture.root.path(),
            &["diff", "--name-only", original_head.trim(), "HEAD"],
        );
        assert!(!committed.contains(".gitignore"));
        assert!(!committed.contains("state/v3/unrelated"));
        assert!(!committed.contains("state/registry/unrelated.json"));
        let staged = git(fixture.root.path(), &["diff", "--cached", "--name-only"]);
        assert!(staged.contains(".gitignore"));
        assert!(staged.contains("state/v3/unrelated"));
        assert!(
            fixture
                .root
                .path()
                .join("state/registry/unrelated.json")
                .is_file()
        );
    }
}

#[test]
fn rolling_back_phase_recovers_idempotently() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "rolling back source\n",
    )
    .expect("source edit");
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
                "convergence_interrupt_after_rollback",
            ),
        ],
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "rolling-back",
        ],
    );
    assert!(
        !output.status.success(),
        "rollback crash passed: {interrupted}"
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
    assert_eq!(journal["phase"], json!("rolling_back"));
    let (output, recovered) = apply(&fixture, &plan, "rolling-back", None);
    assert!(
        output.status.success(),
        "rolling rollback recovery failed: {recovered}"
    );
}
