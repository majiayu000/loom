use std::fs;

use common::run_loom_with_env;

use super::*;

#[path = "recovery_safety/cleanup_ownership.rs"]
mod cleanup_ownership;
#[path = "recovery_safety/recovery_boundaries.rs"]
mod recovery_boundaries;
#[path = "recovery_safety/typed_projection_journal.rs"]
mod typed_projection_journal;

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

fn create_projection_plan(fixture: &Fixture) -> Value {
    let projection = fixture.target.path().join("demo");
    let metadata = fs::symlink_metadata(&projection).expect("projection metadata");
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(&projection).expect("remove projection");
    } else {
        fs::remove_dir_all(&projection).expect("remove projection");
    }
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
    let (output, plan) = plan_converge(fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    plan
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
fn refresh_journal_rejects_missing_prepared_evidence_before_cleanup() {
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
    journal["projections"][0]["state"] = Value::String("prepared".to_string());
    journal["projections"][0]["prepared"] = Value::Null;
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
        "missing prepared evidence recovered: {rejected}"
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
    let plan = create_projection_plan(&fixture);
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

#[cfg(unix)]
#[test]
fn source_committed_create_preserves_external_dangling_symlink() {
    use std::os::unix::fs::symlink;

    let fixture = projected_fixture_with_method("symlink");
    let plan = create_projection_plan(&fixture);
    let fault = "convergence_interrupt_after_source_commit";
    let (output, interrupted) = apply(&fixture, &plan, "dangling-create", Some(fault));
    assert!(!output.status.success(), "fault passed: {interrupted}");
    let projection = fixture.target.path().join("demo");
    let dangling_target = fixture.target.path().join("external-missing-target");
    symlink(&dangling_target, &projection).expect("external dangling symlink");
    let (output, rejected) = apply(&fixture, &plan, "dangling-create", None);
    assert!(
        !output.status.success(),
        "dangling symlink was accepted: {rejected}"
    );
    assert!(
        fs::symlink_metadata(&projection)
            .expect("symlink preserved")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_link(&projection).expect("link target"),
        dangling_target
    );
}

#[test]
fn staged_and_unstaged_projection_registry_drift_are_zero_mutation() {
    for staged in [false, true] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let path = fixture.root.path().join("state/registry/projections.json");
        let mut registry: Value =
            serde_json::from_slice(&fs::read(&path).expect("registry")).expect("parse registry");
        registry["projections"][0]["observed_drift"] = json!(true);
        fs::write(&path, serde_json::to_vec_pretty(&registry).expect("encode")).expect("drift");
        if staged {
            git(
                fixture.root.path(),
                &["add", "state/registry/projections.json"],
            );
        }
        let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let status = git(fixture.root.path(), &["status", "--porcelain"]);
        let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
        let target = snapshot_tree(fixture.target.path());
        let (output, rejected) = apply(&fixture, &plan, "registry-drift", None);
        assert!(
            !output.status.success(),
            "registry drift applied: {rejected}"
        );
        assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), status);
        assert_eq!(
            snapshot_tree(&fixture.root.path().join("skills/demo")),
            source
        );
        assert_eq!(snapshot_tree(fixture.target.path()), target);
        assert!(
            !fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json")
                .exists()
        );
    }
}

#[test]
fn corrupt_projection_artifact_and_index_backup_fail_before_live_mutation() {
    for kind in ["projection", "index"] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            "backup evidence source\n",
        )
        .expect("source edit");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let fault = "convergence_interrupt_after_source_commit";
        let (output, interrupted) = apply(&fixture, &plan, kind, Some(fault));
        assert!(!output.status.success(), "fault passed: {interrupted}");
        let journal_path = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        let journal: Value =
            serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse");
        let backup = if kind == "projection" {
            journal["projections"][0]["prepared"]["staging_path"]
                .as_str()
                .expect("prepared projection artifact")
        } else {
            journal["index_backup"].as_str().expect("index backup")
        };
        if kind == "projection" {
            fs::remove_dir_all(backup).expect("remove prepared artifact");
        } else {
            fs::write(
                fixture.root.path().join("other-valid-index-entry"),
                "other\n",
            )
            .expect("other index entry");
            git(fixture.root.path(), &["add", "other-valid-index-entry"]);
            fs::copy(fixture.root.path().join(".git/index"), backup)
                .expect("replace with another valid index");
        }
        let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
        let target = snapshot_tree(fixture.target.path());
        let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
        let (output, rejected) = apply(&fixture, &plan, kind, None);
        assert!(
            !output.status.success(),
            "corrupt backup recovered: {rejected}"
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
        assert!(journal_path.is_file(), "recovery pointer was deleted");
    }
}

#[test]
fn corrupt_source_backup_fails_before_live_mutation() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance");
    fs::write(
        fixture.target.path().join("demo/details.txt"),
        "projection input\n",
    )
    .expect("projection edit");
    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "plan failed: {plan}");
    let fault = "convergence_interrupt_after_source_commit";
    let (output, interrupted) = apply(&fixture, &plan, "source-backup", Some(fault));
    assert!(!output.status.success(), "fault passed: {interrupted}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse");
    let backup = journal["source_backup"]["backup_path"]
        .as_str()
        .expect("source backup");
    fs::remove_dir_all(backup).expect("remove source backup");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(&fixture.target.path().join("demo"));
    let (output, rejected) = apply(&fixture, &plan, "source-backup", None);
    assert!(
        !output.status.success(),
        "corrupt source backup recovered: {rejected}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(snapshot_tree(&fixture.target.path().join("demo")), target);
    assert!(journal_path.is_file());
}

#[test]
fn declared_cleanup_failure_retains_journal_for_retry() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let fault = "convergence_interrupt_after_prepared";
    let (output, interrupted) = apply(&fixture, &plan, "cleanup-retain", Some(fault));
    assert!(!output.status.success(), "fault passed: {interrupted}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    let (output, failed_cleanup) = run_loom_with_env(
        fixture.root.path(),
        &[(
            "LOOM_CLEANUP_FAULT_INJECT",
            "convergence_fail_declared_cleanup",
        )],
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "cleanup-retain",
        ],
    );
    assert!(
        !output.status.success(),
        "cleanup fault passed: {failed_cleanup}"
    );
    let journal = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    assert!(journal.is_file(), "cleanup failure deleted retry journal");
    let (output, recovered) = apply(&fixture, &plan, "cleanup-retain", None);
    assert!(output.status.success(), "cleanup retry failed: {recovered}");
    assert!(!journal.exists());
}

#[test]
fn source_replacement_recovery_touches_only_source_before_restart() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance");
    fs::write(
        fixture.target.path().join("demo/details.txt"),
        "replacement input\n",
    )
    .expect("projection edit");
    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "plan failed: {plan}");
    fs::write(fixture.root.path().join("unrelated-index"), "staged\n").expect("unrelated");
    git(fixture.root.path(), &["add", "unrelated-index"]);
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let index = git(fixture.root.path(), &["diff", "--cached", "--name-only"]);
    let old_source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target = snapshot_tree(&fixture.target.path().join("demo"));
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let first_fault = "convergence_interrupt_after_source_replacement";
    let (output, interrupted) = apply(&fixture, &plan, "source-replacement", Some(first_fault));
    assert!(
        !output.status.success(),
        "replacement fault passed: {interrupted}"
    );

    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    let (output, restarted) = run_loom_with_env(
        fixture.root.path(),
        &[("LOOM_FAULT_INJECT", "convergence_interrupt_after_prepared")],
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "source-replacement",
        ],
    );
    assert!(
        !output.status.success(),
        "restart fault passed: {restarted}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        git(fixture.root.path(), &["diff", "--cached", "--name-only"]),
        index
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        old_source
    );
    assert_eq!(snapshot_tree(&fixture.target.path().join("demo")), target);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
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
