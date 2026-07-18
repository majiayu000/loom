use super::*;

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
