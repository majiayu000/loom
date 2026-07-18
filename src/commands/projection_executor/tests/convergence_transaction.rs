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
