use super::*;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn finalize_claim_preserves_preclaim_and_postclaim_replacements() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let activate = || {
        fs::create_dir_all(&projection_path).expect("create existing projection");
        fs::write(projection_path.join("keep.txt"), "keep\n").expect("write existing projection");
        let output = execute_projection(
            &fixture.ctx,
            &fixture.paths,
            &fixture.snapshot,
            execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
        )
        .expect("prepare replacement");
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate replacement")
    };

    let activated = activate();
    let backup_path = PathBuf::from(
        activated.rollback_evidence()["backup_path"]
            .as_str()
            .expect("backup path"),
    );
    let (_projection, mut artifact) = activated.into_durable_parts();
    fs::remove_dir_all(&backup_path).expect("remove owned backup");
    fs::create_dir(&backup_path).expect("create preclaim replacement");
    fs::write(backup_path.join("external.txt"), "before\n").expect("write replacement");
    let error = artifact
        .finalize()
        .expect_err("preclaim replacement must be preserved");
    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(backup_path.join("external.txt")).unwrap(),
        "before\n"
    );

    fs::remove_dir_all(&backup_path).expect("remove preclaim replacement");
    fs::remove_dir_all(&projection_path).expect("remove active projection");
    let activated = activate();
    let backup_path = PathBuf::from(
        activated.rollback_evidence()["backup_path"]
            .as_str()
            .expect("backup path"),
    );
    let (_projection, mut artifact) = activated.into_durable_parts();
    artifact
        .finalize_after_claim(|vacated_path| {
            fs::create_dir(vacated_path)?;
            fs::write(vacated_path.join("external.txt"), "after\n")
        })
        .expect("owned claim cleanup");
    assert_eq!(
        fs::read_to_string(backup_path.join("external.txt")).unwrap(),
        "after\n"
    );
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn interrupted_finalize_claim_is_serializable_and_retryable() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create existing projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write existing projection");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare replacement");
    let activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate replacement");
    let (_projection, mut artifact) = activated.into_durable_parts();
    artifact
        .finalize_after_claim(|_| Err(std::io::Error::other("interrupted after claim")))
        .expect_err("injected interruption must preserve claimed artifact");
    let mut artifact: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(artifact).unwrap()).unwrap();

    artifact.finalize().expect("resume claimed cleanup");
    assert!(projection_path.join("details.txt").is_file());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn persisted_finalize_pending_survives_delete_before_journal_update() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).unwrap();
    fs::write(projection_path.join("keep.txt"), "keep\n").unwrap();
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .unwrap();
    let activated = activate_prepared_projection(&fixture.ctx, output.prepared.unwrap()).unwrap();
    let (_projection, mut artifact) = activated.into_durable_parts();
    let mut stale = artifact.clone();

    artifact.prepare_finalize().unwrap();
    let mut persisted: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(&artifact).unwrap()).unwrap();
    let opposite = artifact
        .clone()
        .prepare_rollback()
        .expect_err("finalize pending must reject rollback");
    assert_eq!(opposite.code, ErrorCode::StateCorrupt);
    artifact.cleanup_pending().unwrap();

    stale
        .finalize()
        .expect_err("stale pretransition evidence must not accept missing backup");
    persisted.cleanup_pending().unwrap();
    persisted.cleanup_pending().unwrap();
    assert!(projection_path.join("details.txt").is_file());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn persisted_exchanged_rollback_pending_survives_swap_and_delete() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).unwrap();
    fs::write(projection_path.join("keep.txt"), "keep\n").unwrap();
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .unwrap();
    let activated = activate_prepared_projection(&fixture.ctx, output.prepared.unwrap()).unwrap();
    let (_projection, mut artifact) = activated.into_durable_parts();
    artifact
        .rollback_after_mutation(|| Err(std::io::Error::other("interrupted after exchange")))
        .expect_err("injected interruption leaves stale exchanged evidence");
    let mut artifact: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(&artifact).unwrap()).unwrap();
    artifact
        .prepare_rollback()
        .expect("post-exchange state reconstructs pending cleanup");
    let mut persisted: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(&artifact).unwrap()).unwrap();
    let opposite = artifact
        .clone()
        .prepare_finalize()
        .expect_err("rollback pending must reject finalize");
    assert_eq!(opposite.code, ErrorCode::StateCorrupt);
    artifact.cleanup_pending().unwrap();

    persisted.cleanup_pending().unwrap();
    persisted.cleanup_pending().unwrap();
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!projection_path.join("details.txt").exists());
}

#[test]
fn persisted_created_rollback_pending_survives_rename_and_delete() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .unwrap();
    let activated = activate_prepared_projection(&fixture.ctx, output.prepared.unwrap()).unwrap();
    let (_projection, mut artifact) = activated.into_durable_parts();
    artifact
        .rollback_after_mutation(|| Err(std::io::Error::other("interrupted after rename")))
        .expect_err("injected interruption leaves stale created evidence");
    let mut artifact: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(&artifact).unwrap()).unwrap();
    artifact
        .prepare_rollback()
        .expect("post-rename state reconstructs pending cleanup");
    let mut persisted: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(&artifact).unwrap()).unwrap();
    artifact.cleanup_pending().unwrap();

    persisted.cleanup_pending().unwrap();
    persisted.cleanup_pending().unwrap();
    assert!(!projection_path.exists());
}

#[test]
fn created_finalize_rejects_replaced_live_projection() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .unwrap();
    let activated = activate_prepared_projection(&fixture.ctx, output.prepared.unwrap()).unwrap();
    let (_projection, mut artifact) = activated.into_durable_parts();
    fs::write(projection_path.join("details.txt"), "external\n").unwrap();

    let error = artifact
        .finalize()
        .expect_err("created live replacement must be preserved");
    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(projection_path.join("details.txt")).unwrap(),
        "external\n"
    );
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn prepared_existing_activation_recovers_after_exchange_before_artifact() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).unwrap();
    fs::write(projection_path.join("keep.txt"), "keep\n").unwrap();
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .unwrap();
    let prepared = output.prepared.unwrap();
    let durable = serde_json::to_value(prepared.durable_artifact()).unwrap();
    let interrupted = activate_after_mutation(prepared, || {
        Err(std::io::Error::other("interrupt after exchange"))
    });
    assert!(interrupted.is_err());
    let prepared =
        PreparedProjection::from_durable_artifact(serde_json::from_value(durable).unwrap());

    let mut recovered = activate_prepared_projection(&fixture.ctx, prepared)
        .expect("post-exchange prepared evidence reconstructs activation");
    recovered.rollback().unwrap();
    assert!(projection_path.join("keep.txt").is_file());
    assert!(!projection_path.join("details.txt").exists());
}

#[test]
fn prepared_created_activation_recovers_after_rename_before_artifact() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .unwrap();
    let prepared = output.prepared.unwrap();
    let durable = serde_json::to_value(prepared.durable_artifact()).unwrap();
    let interrupted = activate_after_mutation(prepared, || {
        Err(std::io::Error::other("interrupt after rename"))
    });
    assert!(interrupted.is_err());
    let prepared =
        PreparedProjection::from_durable_artifact(serde_json::from_value(durable).unwrap());

    let mut recovered = activate_prepared_projection(&fixture.ctx, prepared)
        .expect("post-rename prepared evidence reconstructs activation");
    recovered.rollback().unwrap();
    assert!(!projection_path.exists());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn failed_finalize_drop_never_swaps_replaced_backup_into_live_path() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    fs::create_dir_all(&projection_path).expect("create existing projection");
    fs::write(projection_path.join("keep.txt"), "keep\n").expect("write existing projection");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare replacement");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate replacement");
    let backup_path = PathBuf::from(
        activated.rollback_evidence()["backup_path"]
            .as_str()
            .expect("backup path"),
    );
    fs::remove_dir_all(&backup_path).expect("remove owned backup");
    fs::create_dir(&backup_path).expect("create external backup replacement");
    fs::write(backup_path.join("external.txt"), "external\n").expect("write replacement");

    activated
        .finalize()
        .expect_err("replaced backup must fail finalization");
    drop(activated);

    assert!(projection_path.join("details.txt").is_file());
    assert!(!projection_path.join("external.txt").exists());
    assert!(backup_path.join("external.txt").is_file());
}
