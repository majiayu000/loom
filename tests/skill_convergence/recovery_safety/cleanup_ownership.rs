use super::*;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::symlink;

fn projection_input_plan(fixture: &Fixture) -> Value {
    let (output, initial) = plan_converge(fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance");
    fs::write(
        fixture.target.path().join("demo/details.txt"),
        "projection selected for cleanup\n",
    )
    .expect("projection edit");
    let (output, plan) = plan_converge(fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "plan failed: {plan}");
    plan
}

fn owner_path(journal: &Value, surface: &str) -> PathBuf {
    if surface == "source" {
        Path::new(journal["source_staging"].as_str().expect("source staging"))
            .parent()
            .expect("source owner")
            .to_path_buf()
    } else {
        let projections = journal["projections"].as_array().expect("projections");
        PathBuf::from(
            projections
                .last()
                .expect("projection")
                .get("staging_owner")
                .and_then(Value::as_str)
                .expect("projection owner"),
        )
    }
}

#[cfg(unix)]
fn install_owner_attack(owner: &Path, journal: &Value, mode: &str) -> PathBuf {
    let saved = owner.with_extension(format!("saved-{mode}"));
    fs::rename(owner, &saved).expect("save exact owner");
    if mode == "symlink" {
        let external = owner.with_extension(format!("external-{mode}"));
        fs::create_dir(&external).expect("external dir");
        fs::write(external.join("keep"), "external\n").expect("external marker");
        symlink(&external, owner).expect("owner symlink");
    } else {
        fs::create_dir(owner).expect("replacement owner");
        fs::write(owner.join("keep"), "external\n").expect("replacement marker");
        if mode == "different-proof" {
            let plan_id = journal["plan_id"].as_str().expect("plan id");
            fs::write(owner.join(".owner"), format!("{plan_id}\n")).expect("owner marker");
            fs::write(
                owner.join(".reservation-owner"),
                format!("{plan_id}:{}\n", uuid::Uuid::new_v4()),
            )
            .expect("different valid proof");
        }
    }
    saved
}

#[cfg(unix)]
fn restore_exact_owner(owner: &Path, saved: &Path, mode: &str) {
    if mode == "symlink" {
        fs::remove_file(owner).expect("remove owner symlink");
    } else {
        fs::remove_dir_all(owner).expect("remove replacement owner");
    }
    fs::rename(saved, owner).expect("restore exact owner");
}

fn apply_with_cleanup_fault(
    fixture: &Fixture,
    plan: &Value,
    key: &str,
) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("digest");
    run_loom_with_env(
        fixture.root.path(),
        &[(
            "LOOM_CLEANUP_FAULT_INJECT",
            "convergence_interrupt_during_cleanup",
        )],
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

#[cfg(unix)]
#[test]
fn committed_cleanup_rejects_non_exact_present_owners_and_retains_retry_evidence() {
    for surface in ["source", "projection"] {
        for mode in ["missing", "different-proof", "symlink"] {
            let fixture = projected_fixture();
            let plan = if surface == "source" {
                projection_input_plan(&fixture)
            } else {
                add_copy_projection(&fixture, "cleanup-second");
                fs::write(
                    fixture.root.path().join("skills/demo/details.txt"),
                    "committed cleanup projection\n",
                )
                .expect("source edit");
                let (output, plan) = plan_converge(&fixture, &[]);
                assert!(output.status.success(), "plan failed: {plan}");
                plan
            };
            let key = format!("committed-cleanup-{surface}-{mode}");
            let (output, interrupted) = apply_with_cleanup_fault(&fixture, &plan, &key);
            assert!(
                !output.status.success(),
                "cleanup fault passed: {interrupted}"
            );
            let journal_path = fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json");
            let journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
                .expect("parse journal");
            assert_eq!(journal["phase"], json!("committed_cleanup_pending"));
            let owner = owner_path(&journal, surface);
            let saved = install_owner_attack(&owner, &journal, mode);
            let (output, rejected) = apply(&fixture, &plan, &key, None);
            assert!(
                !output.status.success(),
                "cleanup attack passed: {rejected}"
            );
            assert!(journal_path.is_file(), "cleanup attack deleted journal");
            assert_eq!(
                fs::read_to_string(owner.join("keep")).expect("keep"),
                "external\n"
            );
            restore_exact_owner(&owner, &saved, mode);
            let (output, recovered) = apply(&fixture, &plan, &key, None);
            assert!(output.status.success(), "cleanup retry failed: {recovered}");
            assert!(!journal_path.exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn prepared_cleanup_rejects_non_exact_present_owners_and_retains_retry_evidence() {
    for surface in ["source", "projection"] {
        for mode in ["missing", "different-proof", "symlink"] {
            let fixture = projected_fixture();
            let plan = if surface == "source" {
                projection_input_plan(&fixture)
            } else {
                fs::write(
                    fixture.root.path().join("skills/demo/details.txt"),
                    "prepared cleanup projection\n",
                )
                .expect("source edit");
                let (output, plan) = plan_converge(&fixture, &[]);
                assert!(output.status.success(), "plan failed: {plan}");
                plan
            };
            let key = format!("prepared-cleanup-{surface}-{mode}");
            let (output, interrupted) = apply(
                &fixture,
                &plan,
                &key,
                Some("convergence_interrupt_after_prepared"),
            );
            assert!(
                !output.status.success(),
                "prepared fault passed: {interrupted}"
            );
            let journal_path = fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json");
            let journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
                .expect("parse journal");
            let owner = owner_path(&journal, surface);
            let saved = install_owner_attack(&owner, &journal, mode);
            let (output, rejected) = apply(&fixture, &plan, &key, None);
            assert!(
                !output.status.success(),
                "prepared cleanup attack passed: {rejected}"
            );
            assert!(journal_path.is_file(), "prepared cleanup deleted journal");
            assert_eq!(
                fs::read_to_string(owner.join("keep")).expect("keep"),
                "external\n"
            );
            restore_exact_owner(&owner, &saved, mode);
            let (output, recovered) = apply(&fixture, &plan, &key, None);
            assert!(
                output.status.success(),
                "prepared cleanup retry failed: {recovered}"
            );
            assert!(!journal_path.exists());
        }
    }
}
