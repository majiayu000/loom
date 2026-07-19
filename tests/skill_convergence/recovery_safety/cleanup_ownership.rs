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

fn existing_owned_paths(journal: &Value, attacked: &Path) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(
        journal["artifact_root"].as_str().expect("artifact root"),
    )];
    if let Some(staging) = journal["source_staging"].as_str()
        && let Some(owner) = Path::new(staging).parent()
    {
        paths.push(owner.to_path_buf());
    }
    paths.extend(
        journal["projections"]
            .as_array()
            .expect("projections")
            .iter()
            .map(|projection| {
                PathBuf::from(projection["staging_owner"].as_str().expect("staging owner"))
            }),
    );
    paths
        .into_iter()
        .filter(|path| path != attacked && fs::symlink_metadata(path).is_ok())
        .collect()
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

#[test]
fn crash_after_ready_proof_is_retryable_and_retains_exact_attempt() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "reservation publication crash\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "reservation-publication-crash";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_reservation_pending_create"),
    );
    assert!(
        !output.status.success(),
        "reservation publication fault passed: {interrupted}"
    );

    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("journal")).expect("parse journal");
    assert_eq!(journal["phase"], json!("preparing"));
    let attempt = &journal["ownership_attempts"][0];
    assert_eq!(attempt["state"], json!("ready"));
    let candidate = PathBuf::from(attempt["candidate_path"].as_str().expect("candidate"));
    let destination = PathBuf::from(attempt["destination"].as_str().expect("destination"));
    assert!(candidate.join(".ownership-manifest.json").is_file());
    assert!(
        !destination.exists(),
        "ready attempt became public before activation"
    );

    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "reservation retry failed: {recovered}"
    );
    let retained: Value =
        serde_json::from_slice(&fs::read(&journal_path).expect("retained journal"))
            .expect("parse retained journal");
    assert_eq!(retained["phase"], json!("committed_artifacts_retained"));
    assert_eq!(
        retained["ownership_attempts"][0]["state"],
        json!("retained")
    );
    assert!(destination.join(".ownership-manifest.json").is_file());
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
            let retained = existing_owned_paths(&journal, &owner);
            let index_path = PathBuf::from(journal["index_backup"].as_str().expect("index"));
            let index = fs::read(&index_path).expect("index evidence");
            let (output, rejected) = apply(&fixture, &plan, &key, None);
            assert!(
                !output.status.success(),
                "cleanup attack passed: {rejected}"
            );
            assert!(journal_path.is_file(), "cleanup attack deleted journal");
            assert_eq!(fs::read(&index_path).expect("retained index"), index);
            assert!(
                retained
                    .iter()
                    .all(|path| fs::symlink_metadata(path).is_ok())
            );
            assert_eq!(
                fs::read_to_string(owner.join("keep")).expect("keep"),
                "external\n"
            );
            restore_exact_owner(&owner, &saved, mode);
            let (output, recovered) = apply(&fixture, &plan, &key, None);
            assert!(output.status.success(), "cleanup retry failed: {recovered}");
            super::super::skill_convergence_ledger_assertions::assert_exact_retained_ledger(
                &journal_path,
                "committed_artifacts_retained",
            );
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
            let retained = existing_owned_paths(&journal, &owner);
            let index_path = PathBuf::from(journal["index_backup"].as_str().expect("index"));
            let index = fs::read(&index_path).expect("index evidence");
            let (output, rejected) = apply(&fixture, &plan, &key, None);
            assert!(
                !output.status.success(),
                "prepared cleanup attack passed: {rejected}"
            );
            assert!(journal_path.is_file(), "prepared cleanup deleted journal");
            assert_eq!(fs::read(&index_path).expect("retained index"), index);
            assert!(
                retained
                    .iter()
                    .all(|path| fs::symlink_metadata(path).is_ok())
            );
            assert_eq!(
                fs::read_to_string(owner.join("keep")).expect("keep"),
                "external\n"
            );
            restore_exact_owner(&owner, &saved, mode);
            let (output, recovered) = apply(&fixture, &plan, &key, None);
            assert!(
                output.status.success(),
                "prepared cleanup retry failed for {surface}/{mode}: {recovered}"
            );
            super::super::skill_convergence_ledger_assertions::assert_exact_retained_ledger(
                &journal_path,
                "committed_artifacts_retained",
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn nonexact_ready_attempts_block_activation_until_exact_retry() {
    for corruption in ["proof", "manifest", "symlink"] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("ready attempt {corruption}\n"),
        )
        .expect("source edit");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("ready-attempt-{corruption}");
        let (output, interrupted) = apply(
            &fixture,
            &plan,
            &key,
            Some("convergence_interrupt_after_reservation_pending_create"),
        );
        assert!(
            !output.status.success(),
            "ready fault passed: {interrupted}"
        );
        let journal_path = fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json");
        let journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
            .expect("parse journal");
        let attempt = &journal["ownership_attempts"][0];
        assert_eq!(attempt["state"], json!("ready"));
        let candidate = PathBuf::from(attempt["candidate_path"].as_str().expect("candidate"));
        let proof_path = candidate.join(".reservation-owner");
        let manifest_path = candidate.join(".ownership-manifest.json");
        let proof = fs::read(&proof_path).expect("proof");
        let manifest = fs::read(&manifest_path).expect("manifest");
        let saved = candidate.with_extension("saved-exact");
        let external = candidate.with_extension("external");
        match corruption {
            "proof" => fs::write(&proof_path, "foreign-proof\n").expect("replace proof"),
            "manifest" => fs::write(&manifest_path, "{}\n").expect("replace manifest"),
            "symlink" => {
                fs::rename(&candidate, &saved).expect("save candidate");
                fs::create_dir(&external).expect("external target");
                fs::write(external.join("keep"), "external\n").expect("external marker");
                symlink(&external, &candidate).expect("candidate symlink");
            }
            _ => unreachable!(),
        }
        let (output, rejected) = apply(&fixture, &plan, &key, None);
        assert!(
            !output.status.success(),
            "nonexact attempt passed: {rejected}"
        );
        assert!(journal_path.is_file());
        match corruption {
            "proof" => fs::write(&proof_path, proof).expect("restore proof"),
            "manifest" => fs::write(&manifest_path, manifest).expect("restore manifest"),
            "symlink" => {
                assert_eq!(
                    fs::read_to_string(external.join("keep")).expect("external"),
                    "external\n"
                );
                fs::remove_file(&candidate).expect("remove test symlink");
                fs::rename(&saved, &candidate).expect("restore candidate");
            }
            _ => unreachable!(),
        }
        let (output, recovered) = apply(&fixture, &plan, &key, None);
        assert!(output.status.success(), "exact retry failed: {recovered}");
        super::super::skill_convergence_ledger_assertions::assert_exact_retained_ledger(
            &journal_path,
            "committed_artifacts_retained",
        );
    }
}

#[test]
fn resume_prevalidates_all_projection_artifacts_before_any_live_restore() {
    for fault in [
        "convergence_interrupt_after_source_commit",
        "convergence_interrupt_after_projection_swap",
    ] {
        for corruption in ["late-owner", "late-reservation"] {
            let fixture = projected_fixture();
            add_copy_projection(&fixture, "resume-second");
            fs::write(
                fixture.root.path().join("skills/demo/details.txt"),
                format!("resume {fault} {corruption}\n"),
            )
            .expect("source edit");
            let (output, plan) = plan_converge(&fixture, &[]);
            assert!(output.status.success(), "plan failed: {plan}");
            let key = format!("resume-{fault}-{corruption}");
            let (output, interrupted) = apply(&fixture, &plan, &key, Some(fault));
            assert!(
                !output.status.success(),
                "resume fault passed: {interrupted}"
            );
            let journal_path = fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json");
            let journal: Value = serde_json::from_slice(&fs::read(&journal_path).expect("journal"))
                .expect("parse journal");
            let late = journal["projections"]
                .as_array()
                .expect("projections")
                .last()
                .expect("late");
            let late_owner = PathBuf::from(late["staging_owner"].as_str().expect("owner"));
            let plan_id = journal["plan_id"].as_str().expect("plan id");
            let mut saved_owner = None;
            let mut saved_proof = None;
            if corruption == "late-owner" {
                let saved = late_owner.with_extension("resume-saved");
                fs::rename(&late_owner, &saved).expect("save late owner");
                fs::create_dir(&late_owner).expect("replace late owner");
                fs::write(late_owner.join("keep"), "external\n").expect("external owner");
                saved_owner = Some(saved);
            } else {
                let proof_path = late_owner.join(".reservation-owner");
                saved_proof = Some(fs::read(&proof_path).expect("exact proof"));
                fs::write(&proof_path, format!("{plan_id}:{}\n", uuid::Uuid::new_v4()))
                    .expect("late proof mismatch");
            }
            let registry_path = fixture.root.path().join("state/registry/projections.json");
            let registry = fs::read(&registry_path).expect("registry");
            let live = snapshot_tree(fixture.target.path());
            let artifact_root =
                PathBuf::from(journal["artifact_root"].as_str().expect("artifacts"));
            let artifacts = snapshot_tree(&artifact_root);
            let index_path = PathBuf::from(journal["index_backup"].as_str().expect("index"));
            let index = fs::read(&index_path).expect("index evidence");
            let (output, rejected) = apply(&fixture, &plan, &key, None);
            assert!(
                !output.status.success(),
                "late mismatch resumed: {rejected}"
            );
            assert_eq!(fs::read(&registry_path).expect("registry after"), registry);
            assert_eq!(snapshot_tree(fixture.target.path()), live);
            assert_eq!(snapshot_tree(&artifact_root), artifacts);
            assert_eq!(fs::read(&index_path).expect("index after"), index);
            assert!(journal_path.is_file(), "resume mismatch deleted journal");
            if let Some(saved) = saved_owner {
                assert_eq!(
                    fs::read_to_string(late_owner.join("keep")).expect("external owner"),
                    "external\n"
                );
                fs::remove_dir_all(&late_owner).expect("remove replacement owner");
                fs::rename(saved, &late_owner).expect("restore late owner");
            }
            if let Some(proof) = saved_proof {
                fs::write(late_owner.join(".reservation-owner"), proof)
                    .expect("restore exact proof");
            }
            let (output, recovered) = apply(&fixture, &plan, &key, None);
            assert!(output.status.success(), "resume retry failed: {recovered}");
            super::super::skill_convergence_ledger_assertions::assert_exact_retained_ledger(
                &journal_path,
                "committed_artifacts_retained",
            );
        }
    }
}
