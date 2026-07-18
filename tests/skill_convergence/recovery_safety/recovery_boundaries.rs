use super::*;
use std::path::PathBuf;

fn apply_with_rollback_fault(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
    rollback_fault: &str,
) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    run_loom_with_env(
        fixture.root.path(),
        &[
            ("LOOM_FAULT_INJECT", "convergence_after_registry_save"),
            ("LOOM_ROLLBACK_FAULT_INJECT", rollback_fault),
        ],
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
fn every_partial_rollback_boundary_recovers_without_losing_external_index_state() {
    for fault in [
        "convergence_interrupt_after_registry_restore",
        "convergence_interrupt_after_projection_restore",
        "convergence_interrupt_after_source_restore",
        "convergence_interrupt_after_reset",
        "convergence_interrupt_before_index_restore",
        "convergence_interrupt_after_index_restore",
    ] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("rollback boundary {fault}\n"),
        )
        .expect("source edit");
        fs::write(fixture.root.path().join("external-staged"), "staged\n").expect("staged");
        git(fixture.root.path(), &["add", "external-staged"]);
        fs::write(fixture.root.path().join("external-unstaged"), "unstaged\n").expect("unstaged");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("rollback-{fault}");
        let (output, interrupted) = apply_with_rollback_fault(&fixture, &plan, &key, fault);
        assert!(
            !output.status.success(),
            "rollback fault passed: {interrupted}"
        );
        let journal = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        assert!(journal.is_file(), "rollback fault deleted journal");
        let (output, recovered) = apply(&fixture, &plan, &key, None);
        assert!(
            output.status.success(),
            "rollback retry failed for {fault}: {recovered}"
        );
        assert!(
            git(fixture.root.path(), &["diff", "--cached", "--name-only"])
                .contains("external-staged")
        );
        assert_eq!(
            fs::read_to_string(fixture.root.path().join("external-unstaged")).expect("unstaged"),
            "unstaged\n"
        );
    }
}

#[test]
fn source_add_crash_restores_the_exact_original_index_before_retry() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "source add crash\n",
    )
    .expect("source edit");
    fs::write(fixture.root.path().join("external-staged"), "staged\n").expect("staged");
    git(fixture.root.path(), &["add", "external-staged"]);
    fs::write(fixture.root.path().join("external-unstaged"), "unstaged\n").expect("unstaged");
    let original_index = fs::read(fixture.root.path().join(".git/index")).expect("index");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        "source-add",
        Some("convergence_interrupt_after_source_add"),
    );
    assert!(
        !output.status.success(),
        "source add fault passed: {interrupted}"
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
            "source-add",
        ],
    );
    assert!(
        !output.status.success(),
        "restart fault passed: {restarted}"
    );
    assert_eq!(
        fs::read(fixture.root.path().join(".git/index")).expect("restored index"),
        original_index
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("external-unstaged")).expect("unstaged"),
        "unstaged\n"
    );
}

#[cfg(unix)]
#[test]
fn restore_rejects_replaced_or_symlinked_source_and_projection_owner_dirs() {
    use std::os::unix::fs::symlink;

    for surface in ["source", "projection"] {
        for attack in ["replacement", "symlink", "different-valid-proof"] {
            let fixture = projected_fixture();
            let plan = if surface == "source" {
                let (output, initial) = plan_converge(&fixture, &[]);
                assert!(output.status.success(), "initial plan failed: {initial}");
                let instance = initial["data"]["effects"][0]["instance_id"]
                    .as_str()
                    .expect("instance");
                fs::write(
                    fixture.target.path().join("demo/details.txt"),
                    "projection-selected source\n",
                )
                .expect("projection edit");
                let (output, plan) =
                    plan_converge(&fixture, &["--from-projection", "--instance", instance]);
                assert!(output.status.success(), "plan failed: {plan}");
                let (output, interrupted) = apply(
                    &fixture,
                    &plan,
                    &format!("owner-{surface}-{attack}"),
                    Some("convergence_interrupt_after_source_replacement"),
                );
                assert!(
                    !output.status.success(),
                    "source fault passed: {interrupted}"
                );
                plan
            } else {
                fs::write(
                    fixture.root.path().join("skills/demo/details.txt"),
                    "projection rollback owner\n",
                )
                .expect("source edit");
                let (output, plan) = plan_converge(&fixture, &[]);
                assert!(output.status.success(), "plan failed: {plan}");
                let key = format!("owner-{surface}-{attack}");
                let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
                let digest = plan["data"]["plan_digest"].as_str().expect("digest");
                let (output, interrupted) = run_loom_with_env(
                    fixture.root.path(),
                    &[
                        ("LOOM_FAULT_INJECT", "convergence_after_projection_swap"),
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
                        &key,
                    ],
                );
                assert!(
                    !output.status.success(),
                    "projection fault passed: {interrupted}"
                );
                plan
            };

            let journal_path = fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json");
            let journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
                .expect("parse journal");
            let owner = if surface == "source" {
                Path::new(journal["source_staging"].as_str().expect("source staging"))
                    .parent()
                    .expect("source owner")
                    .to_path_buf()
            } else {
                PathBuf::from(
                    journal["projections"][0]["staging_owner"]
                        .as_str()
                        .expect("projection owner"),
                )
            };
            let saved_owner = owner.with_extension("saved-owner");
            fs::rename(&owner, &saved_owner).expect("preserve original owner");
            let external = fixture
                .root
                .path()
                .join(format!("external-owner-{surface}-{attack}"));
            fs::create_dir(&external).expect("external owner");
            fs::write(external.join("keep"), "external\n").expect("external marker");
            if attack == "symlink" {
                symlink(&external, &owner).expect("owner symlink");
            } else {
                fs::create_dir(&owner).expect("replacement owner");
                fs::write(owner.join("keep"), "replacement\n").expect("replacement marker");
                if attack == "different-valid-proof" {
                    let plan_id = journal["plan_id"].as_str().expect("journal plan id");
                    fs::write(owner.join(".owner"), format!("{plan_id}\n"))
                        .expect("forged owner marker");
                    fs::write(
                        owner.join(".reservation-owner"),
                        format!("{plan_id}:{}\n", uuid::Uuid::new_v4()),
                    )
                    .expect("forged valid proof");
                }
            }
            let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
            let status = git(fixture.root.path(), &["status", "--porcelain"]);
            let source_tree = snapshot_tree(&fixture.root.path().join("skills/demo"));
            let target_tree = snapshot_tree(&fixture.target.path().join("demo"));
            let (output, rejected) =
                apply(&fixture, &plan, &format!("owner-{surface}-{attack}"), None);
            assert!(
                !output.status.success(),
                "owner attack recovered: {rejected}"
            );
            assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
            assert_eq!(git(fixture.root.path(), &["status", "--porcelain"]), status);
            assert_eq!(
                snapshot_tree(&fixture.root.path().join("skills/demo")),
                source_tree
            );
            assert_eq!(
                snapshot_tree(&fixture.target.path().join("demo")),
                target_tree
            );
            assert_eq!(
                fs::read_to_string(external.join("keep")).expect("external"),
                "external\n"
            );
            if attack != "symlink" {
                assert_eq!(
                    fs::read_to_string(owner.join("keep")).expect("replacement"),
                    "replacement\n"
                );
            }
            assert!(journal_path.is_file(), "owner attack deleted journal");
        }
    }
}
