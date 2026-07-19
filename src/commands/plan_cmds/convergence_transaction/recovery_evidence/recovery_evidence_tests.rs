use super::*;

struct TestRoot(PathBuf);

impl Drop for TestRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("failed to remove test root '{}': {error}", self.0.display());
        }
    }
}

fn projection_artifact(label: &str, activated: bool) -> ProjectionBackup {
    ProjectionBackup {
        materialized_path: format!("/{label}"),
        backup: None,
        staging_owner: format!("/{label}.owner"),
        owner_proof: format!("{label}-proof"),
        staging_path: format!("/{label}.owner/stage"),
        activated_fingerprint: Some(format!("{label}-active")),
        activated,
        original_fingerprint: Some(format!("{label}-original")),
    }
}

#[test]
fn partial_restore_state_is_inferred_and_retryable() {
    let mut restored = projection_artifact("restored", true);
    let mut failed = projection_artifact("failed", true);
    let mut saw_old = false;

    assert!(
        !reconcile_projection_state(&mut restored, ProjectionState::Old, &mut saw_old)
            .expect("infer restored projection")
    );
    assert!(
        reconcile_projection_state(&mut failed, ProjectionState::New, &mut saw_old)
            .expect("retain failed projection")
    );
    assert!(!restored.is_activated());
    assert!(restored.original_fingerprint.is_none());
    assert!(failed.is_activated());
    assert!(failed.original_fingerprint.is_some());
}

#[test]
fn unrecorded_new_projection_after_old_remains_stale() {
    let mut artifact = projection_artifact("unrecorded", false);
    let error = reconcile_projection_state(&mut artifact, ProjectionState::New, &mut true)
        .expect_err("unrecorded out-of-order projection must fail");
    assert_eq!(error.code, ErrorCode::DependencyConflict);
}

#[test]
fn equal_content_partial_restore_uses_ownership_identity() {
    let mut restored = projection_artifact("equal-content-restored", true);
    let mut failed = projection_artifact("equal-content-failed", true);
    let restored_identity = restored.original_fingerprint.clone().expect("original");
    let failed_identity = failed.activated_fingerprint.clone().expect("activated");
    let mut saw_old = false;

    let restored_state =
        projection_identity_state(&restored, &restored_identity).expect("restored identity");
    assert!(
        !reconcile_projection_state(&mut restored, restored_state, &mut saw_old)
            .expect("infer restored equal-content projection")
    );
    let failed_state =
        projection_identity_state(&failed, &failed_identity).expect("activated identity");
    assert!(
        reconcile_projection_state(&mut failed, failed_state, &mut saw_old)
            .expect("retain failed equal-content projection")
    );
    assert!(restored.original_fingerprint.is_none());
    assert!(failed.original_fingerprint.is_some());

    let error = match projection_identity_state(&failed, "unknown-identity") {
        Ok(_) => panic!("unknown equal-content identity must fail"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::DependencyConflict);
}

#[test]
fn equal_content_backup_copy_restore_uses_retained_exchange_evidence() {
    let root = TestRoot(std::env::temp_dir().join(format!(
        "loom-equal-content-restore-{}",
        uuid::Uuid::new_v4().simple()
    )));
    let live = root.0.join("target/demo");
    let held_original = root.0.join("held-original");
    let backup_path = root.0.join("backup");
    fs::create_dir_all(&live).expect("create original projection");
    fs::write(live.join("SKILL.md"), "same\n").expect("write original projection");
    let original = convergence_projection_fingerprint(&live).expect("original fingerprint");
    fs::create_dir_all(&backup_path).expect("create durable backup");
    fs::write(backup_path.join("SKILL.md"), "same\n").expect("write durable backup");

    fs::rename(&live, &held_original).expect("hold original inode");
    fs::create_dir_all(&live).expect("create activated projection");
    fs::write(live.join("SKILL.md"), "same\n").expect("write activated projection");
    let activated = convergence_projection_fingerprint(&live).expect("activated fingerprint");
    assert_ne!(activated, original);

    let plan_id = "plan-equal-content-copy-restore";
    let owner = root.0.join("target/.loom-projection-stage-owner");
    let owner_proof = new_owner_proof(plan_id);
    reserve_owned_dir(&owner, plan_id, &owner_proof).expect("reserve staging owner");
    let staging = owner.join("stage");
    let mut artifact = ProjectionBackup {
        materialized_path: live.display().to_string(),
        backup: Some(json!({
            "kind": "dir",
            "original_path": live.display().to_string(),
            "backup_path": backup_path.display().to_string(),
            "view": "copy",
        })),
        staging_owner: owner.display().to_string(),
        owner_proof,
        staging_path: staging.display().to_string(),
        activated_fingerprint: Some(activated.clone()),
        activated: true,
        original_fingerprint: Some(original.clone()),
    };

    super::super::projection_recovery::restore_projection_from_evidence(&artifact, plan_id)
        .expect("restore from durable backup copy");
    let restored = convergence_projection_fingerprint(&live).expect("restored fingerprint");
    assert_ne!(restored, original);
    assert_ne!(restored, activated);
    assert_eq!(
        convergence_projection_fingerprint(&staging).expect("retained activated fingerprint"),
        activated
    );
    let state = same_content_projection_state(&live, &artifact)
        .expect("retained exchange proves restored state");
    let mut saw_old = false;
    assert!(
        !reconcile_projection_state(&mut artifact, state, &mut saw_old)
            .expect("reconcile restored copy")
    );

    fs::write(live.join("external.txt"), "external\n").expect("change restored projection");
    let error = match same_content_projection_state(&live, &artifact) {
        Ok(_) => panic!("changed restored projection must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::DependencyConflict);
}
