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
