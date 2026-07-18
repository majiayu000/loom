use super::*;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn rollback_preserves_concurrent_live_change_and_is_retryable() {
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
    let evidence = activated.rollback_evidence();
    let backup_path = PathBuf::from(evidence["backup_path"].as_str().expect("backup path"));
    fs::write(projection_path.join("concurrent.txt"), "concurrent\n")
        .expect("write concurrent live change");

    let error = activated
        .rollback()
        .expect_err("rollback must preserve concurrent live changes");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert_eq!(
        fs::read_to_string(projection_path.join("concurrent.txt")).unwrap(),
        "concurrent\n"
    );
    assert!(backup_path.join("keep.txt").is_file());

    fs::remove_file(projection_path.join("concurrent.txt")).expect("resolve live conflict");
    activated.rollback().expect("retry rollback");
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!backup_path.exists());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn rollback_preserves_concurrent_backup_change_and_is_retryable() {
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
    let evidence = activated.rollback_evidence();
    let backup_path = PathBuf::from(evidence["backup_path"].as_str().expect("backup path"));
    fs::write(backup_path.join("concurrent.txt"), "concurrent\n").expect("change rollback backup");

    let error = activated
        .rollback()
        .expect_err("rollback must preserve a changed backup");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert!(projection_path.join("details.txt").is_file());
    assert!(backup_path.join("keep.txt").is_file());
    assert!(backup_path.join("concurrent.txt").is_file());

    fs::remove_file(backup_path.join("concurrent.txt")).expect("resolve backup conflict");
    activated.rollback().expect("retry rollback");
    assert_eq!(
        fs::read_to_string(projection_path.join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!backup_path.exists());
}

#[test]
fn created_rollback_preserves_concurrent_live_change_and_is_retryable() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare new projection");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate new projection");
    fs::write(projection_path.join("concurrent.txt"), "concurrent\n")
        .expect("write concurrent live change");

    let error = activated
        .rollback()
        .expect_err("rollback must preserve concurrent created projection changes");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert_eq!(
        fs::read_to_string(projection_path.join("concurrent.txt")).unwrap(),
        "concurrent\n"
    );

    fs::remove_file(projection_path.join("concurrent.txt")).expect("resolve live conflict");
    activated.rollback().expect("retry created rollback");
    assert!(!projection_path.exists());
}

#[test]
fn created_rollback_preserves_concurrent_empty_directory_and_is_retryable() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare new projection");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate new projection");
    let concurrent_dir = projection_path.join("concurrent-empty");
    fs::create_dir(&concurrent_dir).expect("create concurrent empty directory");

    let error = activated
        .rollback()
        .expect_err("rollback must preserve a concurrent empty directory");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert!(concurrent_dir.is_dir());

    fs::remove_dir(&concurrent_dir).expect("resolve empty directory conflict");
    activated.rollback().expect("retry created rollback");
    assert!(!projection_path.exists());
}

#[cfg(unix)]
#[test]
fn created_rollback_preserves_concurrent_mode_change_and_is_retryable() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare new projection");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate new projection");
    let details_path = projection_path.join("details.txt");
    let original_mode = fs::symlink_metadata(&details_path)
        .expect("read original mode")
        .permissions()
        .mode();
    fs::set_permissions(
        &details_path,
        fs::Permissions::from_mode(original_mode ^ 0o100),
    )
    .expect("change projected file mode");

    let error = activated
        .rollback()
        .expect_err("rollback must preserve a concurrent mode change");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert_eq!(
        fs::symlink_metadata(&details_path)
            .expect("read changed mode")
            .permissions()
            .mode(),
        original_mode ^ 0o100
    );

    fs::set_permissions(&details_path, fs::Permissions::from_mode(original_mode))
        .expect("resolve mode conflict");
    activated.rollback().expect("retry created rollback");
    assert!(!projection_path.exists());
}

#[cfg(unix)]
#[test]
fn created_rollback_preserves_concurrent_xattr_change_and_is_retryable() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare new projection");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate new projection");
    let details_path = projection_path.join("details.txt");
    let attribute = if cfg!(target_os = "macos") {
        "com.loom.concurrent-test"
    } else {
        "user.loom.concurrent-test"
    };
    match xattr::set(&details_path, attribute, b"concurrent") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => return,
        Err(error) => panic!("set concurrent xattr: {error}"),
    }

    let error = activated
        .rollback()
        .expect_err("rollback must preserve a concurrent xattr change");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert_eq!(
        xattr::get(&details_path, attribute).expect("read changed xattr"),
        Some(b"concurrent".to_vec())
    );

    xattr::remove(&details_path, attribute).expect("resolve xattr conflict");
    activated.rollback().expect("retry created rollback");
    assert!(!projection_path.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn created_rollback_preserves_concurrent_acl_change_and_is_retryable() {
    use std::process::Command;

    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let output = execute_projection(
        &fixture.ctx,
        &fixture.paths,
        &fixture.snapshot,
        execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
    )
    .expect("prepare new projection");
    let mut activated =
        activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
            .expect("activate new projection");
    let details_path = projection_path.join("details.txt");
    let add_status = Command::new("chmod")
        .args(["+a", "everyone allow read"])
        .arg(&details_path)
        .status()
        .expect("run chmod +a");
    assert!(add_status.success(), "add test ACL");

    let error = activated
        .rollback()
        .expect_err("rollback must preserve a concurrent ACL change");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert!(details_path.is_file());

    let remove_status = Command::new("chmod")
        .args(["-a#", "0"])
        .arg(&details_path)
        .status()
        .expect("run chmod -a#");
    assert!(remove_status.success(), "remove test ACL");
    activated.rollback().expect("retry created rollback");
    assert!(!projection_path.exists());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn finalize_preserves_changed_backup_and_is_retryable() {
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
    let evidence = activated.rollback_evidence();
    let backup_path = PathBuf::from(evidence["backup_path"].as_str().expect("backup path"));
    fs::write(backup_path.join("concurrent.txt"), "concurrent\n")
        .expect("change rollback artifact");

    let error = activated
        .finalize()
        .expect_err("finalize must preserve changed backup");

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert!(backup_path.join("concurrent.txt").is_file());
    assert!(projection_path.join("details.txt").is_file());

    fs::remove_file(backup_path.join("concurrent.txt")).expect("resolve backup conflict");
    activated.finalize().expect("retry finalize");
    assert!(!backup_path.exists());
    assert!(projection_path.join("details.txt").is_file());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn finalize_preserves_changed_live_projection_for_created_and_replaced_paths() {
    for replace_existing in [false, true] {
        let fixture = convergence_projection_fixture();
        let projection_path = fixture.root.join("live/copy/demo");
        if replace_existing {
            fs::create_dir_all(&projection_path).expect("create existing projection");
            fs::write(projection_path.join("keep.txt"), "keep\n")
                .expect("write existing projection");
        }
        let output = execute_projection(
            &fixture.ctx,
            &fixture.paths,
            &fixture.snapshot,
            execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
        )
        .expect("prepare projection");
        let mut activated =
            activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
                .expect("activate projection");
        fs::write(projection_path.join("concurrent.txt"), "concurrent\n")
            .expect("change live projection");

        let error = activated
            .finalize()
            .expect_err("finalize must preserve a changed live projection");

        assert_eq!(error.code, ErrorCode::ProjectionConflict);
        assert_eq!(error.details["recovery_required"], true);
        assert!(projection_path.join("concurrent.txt").is_file());
        fs::remove_file(projection_path.join("concurrent.txt")).expect("resolve live conflict");
        activated.finalize().expect("retry finalize");
        assert!(projection_path.join("details.txt").is_file());
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn finalize_rejects_rollback_pending_cleanup() {
    for replace_existing in [false, true] {
        let fixture = convergence_projection_fixture();
        let projection_path = fixture.root.join("live/copy/demo");
        if replace_existing {
            fs::create_dir_all(&projection_path).expect("create existing projection");
            fs::write(projection_path.join("keep.txt"), "keep\n")
                .expect("write existing projection");
        }
        let output = execute_projection(
            &fixture.ctx,
            &fixture.paths,
            &fixture.snapshot,
            execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
        )
        .expect("prepare projection");
        let mut activated =
            activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
                .expect("activate projection");
        let evidence = activated.rollback_evidence();
        let artifact_path = PathBuf::from(
            evidence
                .get(if replace_existing {
                    "backup_path"
                } else {
                    "rollback_path"
                })
                .and_then(Value::as_str)
                .expect("rollback artifact path"),
        );
        activated.fail_cleanup_once_for_test();

        let rollback_error = activated
            .rollback()
            .expect_err("rollback cleanup fault must remain retryable");
        assert_eq!(rollback_error.details["recovery_required"], true);
        if replace_existing {
            assert!(projection_path.join("keep.txt").is_file());
        } else {
            assert!(!projection_path.exists());
        }
        assert!(artifact_path.join("details.txt").is_file());

        let finalize_error = activated
            .finalize()
            .expect_err("finalize must reject rollback-pending cleanup");
        assert_eq!(finalize_error.code, ErrorCode::ProjectionConflict);
        assert_eq!(finalize_error.details["recovery_required"], true);
        activated.rollback().expect("finish rollback cleanup");
        assert!(!artifact_path.exists());
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn rollback_accepts_already_removed_pending_cleanup_artifact() {
    for replace_existing in [false, true] {
        let fixture = convergence_projection_fixture();
        let projection_path = fixture.root.join("live/copy/demo");
        if replace_existing {
            fs::create_dir_all(&projection_path).expect("create existing projection");
            fs::write(projection_path.join("keep.txt"), "keep\n")
                .expect("write existing projection");
        }
        let output = execute_projection(
            &fixture.ctx,
            &fixture.paths,
            &fixture.snapshot,
            execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
        )
        .expect("prepare projection");
        let mut activated =
            activate_prepared_projection(&fixture.ctx, output.prepared.expect("staging artifact"))
                .expect("activate projection");
        let evidence = activated.rollback_evidence();
        let artifact_path = PathBuf::from(
            evidence
                .get(if replace_existing {
                    "backup_path"
                } else {
                    "rollback_path"
                })
                .and_then(Value::as_str)
                .expect("rollback artifact path"),
        );
        activated.fail_cleanup_once_for_test();
        activated
            .rollback()
            .expect_err("rollback cleanup fault must preserve retry state");
        fs::remove_dir_all(&artifact_path).expect("simulate external artifact cleanup");

        activated
            .rollback()
            .expect("missing pending cleanup artifact is already clean");

        assert!(!artifact_path.exists());
        let cleared_error = activated
            .rollback()
            .expect_err("successful retry must clear rollback state");
        assert_eq!(cleared_error.code, ErrorCode::InternalError);
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn activation_rejects_live_change_after_prepare() {
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
    let prepared = output.prepared.expect("staging artifact");
    let staging_path = prepared.staging_path().to_path_buf();
    fs::write(projection_path.join("concurrent.txt"), "concurrent\n")
        .expect("change live projection after prepare");

    let error = match activate_prepared_projection(&fixture.ctx, prepared) {
        Ok(_) => panic!("changed live projection must block activation"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(
        fs::read_to_string(projection_path.join("concurrent.txt")).unwrap(),
        "concurrent\n"
    );
    assert!(projection_path.join("keep.txt").is_file());
    assert!(!projection_path.join("details.txt").exists());
    assert!(!staging_path.exists());
}

#[test]
fn tampered_prepared_artifact_is_preserved_on_drop_and_discard() {
    let fixture = convergence_projection_fixture();
    let projection_path = fixture.root.join("live/copy/demo");
    let prepare = || {
        execute_projection(
            &fixture.ctx,
            &fixture.paths,
            &fixture.snapshot,
            execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone()),
        )
        .expect("prepare projection")
        .prepared
        .expect("prepared artifact")
    };

    let prepared = prepare();
    let staging_path = prepared.staging_path().to_path_buf();
    let claim_path = staging_path.with_file_name(format!(
        "{}.prepared-cleanup-claim",
        staging_path.file_name().unwrap().to_string_lossy()
    ));
    fs::remove_dir_all(&staging_path).expect("replace prepared staging");
    fs::create_dir(&staging_path).expect("create external replacement");
    fs::write(staging_path.join("external.txt"), "external\n").expect("write replacement");
    drop(prepared);
    assert_eq!(
        fs::read_to_string(claim_path.join("external.txt")).unwrap(),
        "external\n"
    );
    fs::remove_dir_all(&claim_path).expect("remove first replacement");

    let prepared = prepare();
    let staging_path = prepared.staging_path().to_path_buf();
    let claim_path = staging_path.with_file_name(format!(
        "{}.prepared-cleanup-claim",
        staging_path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(staging_path.join("external.txt"), "changed\n").expect("tamper staging");
    let error = discard_prepared_projection(prepared).expect_err("tamper must be preserved");
    assert_eq!(error.code, ErrorCode::ProjectionConflict);
    assert_eq!(error.details["recovery_required"], true);
    assert!(claim_path.join("external.txt").is_file());
}

#[test]
fn caller_selected_source_and_staging_round_trip_as_durable_evidence() {
    let fixture = convergence_projection_fixture();
    let selected_source = fixture.root.join("selected-source");
    fs::create_dir(&selected_source).expect("create selected source");
    fs::write(selected_source.join("details.txt"), "selected\n").expect("write selected source");
    let projection_path = fixture.root.join("live/copy/demo");
    let supplied_staging = projection_path
        .parent()
        .expect("projection parent")
        .join(".loom-projection-stage-journal-owned");
    let mut input = execution_input(&fixture, ProjectionMethod::Copy, projection_path.clone());
    input.source_path = Some(selected_source.clone());
    input.staging_path = Some(supplied_staging.clone());

    let output = execute_projection(&fixture.ctx, &fixture.paths, &fixture.snapshot, input)
        .expect("prepare caller-owned projection");
    let prepared = output.prepared.expect("prepared artifact");
    assert_eq!(prepared.staging_path(), supplied_staging);
    assert_eq!(
        fs::read_to_string(supplied_staging.join("details.txt")).unwrap(),
        "selected\n"
    );
    let artifact = prepared.into_durable_artifact();
    let artifact: PreparedProjectionArtifact =
        serde_json::from_value(serde_json::to_value(artifact).unwrap()).unwrap();
    assert_eq!(artifact.source_path, selected_source);
    assert_eq!(artifact.staging_path, supplied_staging);
    let reconstructed = PreparedProjection::from_durable_artifact(artifact);

    discard_prepared_projection(reconstructed).expect("discard reconstructed staging");
    assert!(!supplied_staging.exists());
    assert!(!projection_path.exists());
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[test]
fn durable_activation_round_trip_restores_projection() {
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
    let (projection, artifact) = activated.into_durable_parts();
    let projection: RegistryProjectionInstance =
        serde_json::from_value(serde_json::to_value(projection).unwrap()).unwrap();
    let artifact: ProjectionRollbackArtifact =
        serde_json::from_value(serde_json::to_value(artifact).unwrap()).unwrap();
    let mut resumed = ProjectionActivationOutput::from_durable_parts(projection, artifact);

    resumed.rollback().expect("rollback reconstructed artifact");
    assert!(projection_path.join("keep.txt").is_file());
    assert!(!projection_path.join("details.txt").exists());
}

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
