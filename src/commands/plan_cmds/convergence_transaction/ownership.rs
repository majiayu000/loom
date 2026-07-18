use super::*;
use std::fs::OpenOptions;
use std::io::Write;

const OWNER_FILE: &str = ".owner";
const RESERVATION_PROOF_FILE: &str = ".reservation-owner";

pub(super) fn validate_owned_staging(
    live: &Path,
    staging: &Path,
    plan_id: &str,
    expected_proof: &str,
) -> std::result::Result<(), CommandFailure> {
    let owner = staging
        .parent()
        .ok_or_else(|| ownership_failure("transaction staging has no owner directory"))?;
    if !owner_dir_is_exact(owner, plan_id, expected_proof) {
        return Err(ownership_failure(
            "transaction staging owner proof does not match the journal",
        ));
    }
    let live_parent = live
        .parent()
        .ok_or_else(|| ownership_failure("transaction live path has no parent"))?;
    if !same_filesystem(owner, live_parent)? {
        return Err(ownership_failure(
            "transaction staging and live path are on different filesystems",
        ));
    }
    Ok(())
}

pub(super) fn owner_dir_is_exact(owner: &Path, plan_id: &str, expected_proof: &str) -> bool {
    fs::symlink_metadata(owner)
        .ok()
        .is_some_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        && exact_regular_file(&owner.join(OWNER_FILE), plan_id)
        && exact_regular_file(&owner.join(RESERVATION_PROOF_FILE), expected_proof)
        && owner_proof_is_valid(plan_id, expected_proof)
}

pub(super) fn owner_proof_is_valid(plan_id: &str, proof: &str) -> bool {
    let Some(nonce) = proof.strip_prefix(&format!("{plan_id}:")) else {
        return false;
    };
    uuid::Uuid::parse_str(nonce)
        .ok()
        .is_some_and(|parsed| parsed.hyphenated().to_string() == nonce)
}

fn exact_regular_file(path: &Path, expected: &str) -> bool {
    fs::symlink_metadata(path).ok().is_some_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && fs::read_to_string(path)
                .ok()
                .is_some_and(|value| value.trim() == expected)
    })
}

pub(super) fn cleanup_owned_dir(
    path: &Path,
    plan_id: &str,
    owner_proof: &str,
    errors: &mut Vec<Value>,
) {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => push_rollback_error(errors, "inspect_owned_transaction_artifact", err),
        Ok(_) if owner_dir_is_exact(path, plan_id, owner_proof) => {
            if let Err(err) = remove_path_if_exists(path) {
                push_rollback_error(errors, "remove_owned_transaction_artifact", err);
            }
        }
        Ok(_) => push_rollback_error(
            errors,
            "validate_owned_transaction_artifact",
            "present transaction artifact does not match its journal ownership proof",
        ),
    }
    cleanup_reservation(path, plan_id, owner_proof, errors);
}

pub(super) fn reservation_paths(
    path: &Path,
    plan_id: &str,
) -> std::result::Result<(PathBuf, PathBuf), CommandFailure> {
    let parent = path.parent().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "artifact path has no parent")
    })?;
    let name = path.file_name().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "artifact path has no file name")
    })?;
    let name = name.to_string_lossy();
    Ok((
        parent.join(format!(".{name}.reservation-{plan_id}")),
        parent.join(format!(".{name}.staging-{plan_id}")),
    ))
}

fn reservation_pending_path(
    path: &Path,
    plan_id: &str,
    expected_proof: &str,
) -> std::result::Result<PathBuf, CommandFailure> {
    let nonce = expected_proof
        .strip_prefix(&format!("{plan_id}:"))
        .filter(|nonce| uuid::Uuid::parse_str(nonce).is_ok())
        .ok_or_else(|| ownership_failure("journal owner proof is invalid"))?;
    let parent = path.parent().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "artifact path has no parent")
    })?;
    let name = path.file_name().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "artifact path has no file name")
    })?;
    Ok(parent.join(format!(
        ".{}.reservation-pending-{nonce}",
        name.to_string_lossy()
    )))
}

pub(super) fn reserve_owned_dir(
    path: &Path,
    plan_id: &str,
    reservation_proof: &str,
) -> std::result::Result<(), CommandFailure> {
    if !owner_proof_is_valid(plan_id, reservation_proof) {
        return Err(ownership_failure("journal owner proof is invalid"));
    }
    let (reservation, staging) = reservation_paths(path, plan_id)?;
    publish_reservation_token(path, &reservation, plan_id, reservation_proof)?;
    if let Err(err) = fs::create_dir(&staging) {
        let mut cleanup_errors = Vec::new();
        cleanup_reservation(path, plan_id, reservation_proof, &mut cleanup_errors);
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("artifact staging collision at {}: {err}", staging.display()),
        )
        .with_rollback_errors(cleanup_errors));
    }
    let proof_path = staging.join(RESERVATION_PROOF_FILE);
    write_new_synced(&proof_path, reservation_proof)?;
    maybe_skill_fault("convergence_interrupt_after_owner_root_creation")?;
    write_new_synced(&staging.join(OWNER_FILE), plan_id)?;
    maybe_skill_fault("convergence_interrupt_after_owner_marker_write")?;
    if let Err(err) = rename_no_replace_atomic(&staging, path) {
        let mut cleanup_errors = Vec::new();
        cleanup_reservation(path, plan_id, reservation_proof, &mut cleanup_errors);
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "artifact reservation collision at {}: {err}",
                path.display()
            ),
        )
        .with_rollback_errors(cleanup_errors));
    }
    fs::remove_file(&reservation).map_err(map_io)
}

fn publish_reservation_token(
    path: &Path,
    reservation: &Path,
    plan_id: &str,
    reservation_proof: &str,
) -> std::result::Result<(), CommandFailure> {
    match fs::symlink_metadata(reservation) {
        Ok(_) if exact_regular_file(reservation, reservation_proof) => return Ok(()),
        Ok(_) => {
            return Err(ownership_failure(&format!(
                "artifact reservation collision at {}",
                path.display()
            )));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(map_io(err)),
    }
    let pending = reservation_pending_path(path, plan_id, reservation_proof)?;
    match fs::symlink_metadata(&pending) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(&pending).map_err(map_io)?;
        }
        Ok(_) => {
            return Err(ownership_failure(
                "reservation pending entry is not an owned regular file",
            ));
        }
        Err(err) => return Err(map_io(err)),
    }
    let mut token = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(map_io)?;
    maybe_skill_fault("convergence_interrupt_after_reservation_pending_create")?;
    if let Err(error) = writeln!(token, "{reservation_proof}").and_then(|_| token.sync_all()) {
        drop(token);
        let cleanup = fs::remove_file(&pending);
        return Err(match cleanup {
            Ok(()) => map_io(error),
            Err(cleanup) => CommandFailure::new(
                ErrorCode::IoError,
                format!(
                    "{error}; additionally failed to remove reservation pending file: {cleanup}"
                ),
            ),
        });
    }
    drop(token);
    if let Err(error) = rename_no_replace_atomic(&pending, reservation) {
        if exact_regular_file(reservation, reservation_proof) {
            remove_regular_pending(&pending)?;
            return Ok(());
        }
        let mut failure = CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "artifact reservation collision at {}: {error}",
                path.display()
            ),
        );
        if let Err(cleanup) = remove_regular_pending(&pending) {
            failure = failure.with_rollback_errors(vec![json!({
                "step": "remove_failed_reservation_pending",
                "message": cleanup.message,
            })]);
        }
        return Err(failure);
    }
    Ok(())
}

fn remove_regular_pending(path: &Path) -> std::result::Result<(), CommandFailure> {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            fs::remove_file(path).map_err(map_io)
        }
        Ok(_) => Err(ownership_failure(
            "reservation pending entry is not an owned regular file",
        )),
        Err(err) => Err(map_io(err)),
    }
}

fn write_new_synced(path: &Path, contents: &str) -> std::result::Result<(), CommandFailure> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(map_io)?;
    writeln!(file, "{contents}").map_err(map_io)?;
    file.sync_all().map_err(map_io)
}

pub(super) fn cleanup_reservation(
    path: &Path,
    plan_id: &str,
    expected_proof: &str,
    errors: &mut Vec<Value>,
) {
    let Ok((reservation, staging)) = reservation_paths(path, plan_id) else {
        push_rollback_error(
            errors,
            "resolve_artifact_reservation",
            "artifact path has no parent or file name",
        );
        return;
    };
    match reservation_pending_path(path, plan_id, expected_proof) {
        Ok(pending) => cleanup_pending_entry(&pending, errors),
        Err(err) => push_rollback_error(errors, "resolve_reservation_pending", err.message),
    }
    cleanup_proof_entry(
        &staging,
        &staging.join(RESERVATION_PROOF_FILE),
        expected_proof,
        true,
        "reservation staging",
        errors,
    );
    cleanup_proof_entry(
        &reservation,
        &reservation,
        expected_proof,
        false,
        "reservation token",
        errors,
    );
}

fn cleanup_pending_entry(path: &Path, errors: &mut Vec<Value>) {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            if let Err(err) = fs::remove_file(path) {
                push_rollback_error(errors, "remove_reservation_pending", err);
            }
        }
        Ok(_) => push_rollback_error(
            errors,
            "validate_reservation_pending",
            "reservation pending entry is not an owned regular file",
        ),
        Err(err) => push_rollback_error(errors, "inspect_reservation_pending", err),
    }
}

fn cleanup_proof_entry(
    entry: &Path,
    proof_path: &Path,
    expected_proof: &str,
    require_directory: bool,
    label: &str,
    errors: &mut Vec<Value>,
) {
    match fs::symlink_metadata(entry) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => push_rollback_error(errors, "inspect_artifact_reservation", err),
        Ok(_) if proof_entry_is_exact(entry, proof_path, expected_proof, require_directory) => {
            if let Err(err) = remove_path_if_exists(entry) {
                push_rollback_error(errors, "remove_artifact_reservation", err);
            }
        }
        Ok(_) => push_rollback_error(
            errors,
            "validate_artifact_reservation",
            format!("present {label} does not match its journal ownership proof"),
        ),
    }
}

pub(super) fn validate_transaction_artifacts(journal: &TransactionJournal) -> Vec<Value> {
    let mut errors = Vec::new();
    for projection in &journal.projections {
        validate_cleanup_entry(
            Path::new(&projection.staging_owner),
            &journal.plan_id,
            &projection.owner_proof,
            &mut errors,
        );
    }
    if let Some(staging) = journal.source_staging.as_deref()
        && let Some(owner) = Path::new(staging).parent()
        && let Some(proof) = journal.source_owner_proof.as_deref()
    {
        validate_cleanup_entry(owner, &journal.plan_id, proof, &mut errors);
    }
    validate_cleanup_entry(
        Path::new(&journal.artifact_root),
        &journal.plan_id,
        &journal.artifact_owner_proof,
        &mut errors,
    );
    errors
}

fn validate_cleanup_entry(path: &Path, plan_id: &str, proof: &str, errors: &mut Vec<Value>) {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => push_rollback_error(errors, "inspect_owned_transaction_artifact", err),
        Ok(_) if owner_dir_is_exact(path, plan_id, proof) => {}
        Ok(_) => push_rollback_error(
            errors,
            "validate_owned_transaction_artifact",
            "present transaction artifact does not match its journal ownership proof",
        ),
    }
    let Ok((reservation, staging)) = reservation_paths(path, plan_id) else {
        push_rollback_error(
            errors,
            "resolve_artifact_reservation",
            "artifact path has no parent or file name",
        );
        return;
    };
    match reservation_pending_path(path, plan_id, proof) {
        Ok(pending) => validate_pending_entry(&pending, errors),
        Err(err) => push_rollback_error(errors, "resolve_reservation_pending", err.message),
    }
    validate_proof_entry(
        &staging,
        &staging.join(RESERVATION_PROOF_FILE),
        proof,
        true,
        "reservation staging",
        errors,
    );
    validate_proof_entry(
        &reservation,
        &reservation,
        proof,
        false,
        "reservation token",
        errors,
    );
}

fn validate_pending_entry(path: &Path, errors: &mut Vec<Value>) {
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => push_rollback_error(
            errors,
            "validate_reservation_pending",
            "reservation pending entry is not an owned regular file",
        ),
        Err(err) => push_rollback_error(errors, "inspect_reservation_pending", err),
    }
}

fn validate_proof_entry(
    entry: &Path,
    proof_path: &Path,
    expected_proof: &str,
    require_directory: bool,
    label: &str,
    errors: &mut Vec<Value>,
) {
    match fs::symlink_metadata(entry) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => push_rollback_error(errors, "inspect_artifact_reservation", err),
        Ok(_) if proof_entry_is_exact(entry, proof_path, expected_proof, require_directory) => {}
        Ok(_) => push_rollback_error(
            errors,
            "validate_artifact_reservation",
            format!("present {label} does not match its journal ownership proof"),
        ),
    }
}

fn proof_entry_is_exact(
    entry: &Path,
    proof_path: &Path,
    expected_proof: &str,
    require_directory: bool,
) -> bool {
    let kind_matches = fs::symlink_metadata(entry).ok().is_some_and(|metadata| {
        !metadata.file_type().is_symlink()
            && if require_directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            }
    });
    kind_matches && exact_regular_file(proof_path, expected_proof)
}

#[cfg(unix)]
fn same_filesystem(left: &Path, right: &Path) -> std::result::Result<bool, CommandFailure> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata(left).map_err(map_io)?.dev() == fs::metadata(right).map_err(map_io)?.dev())
}

#[cfg(not(unix))]
fn same_filesystem(_left: &Path, _right: &Path) -> std::result::Result<bool, CommandFailure> {
    Err(ownership_failure(
        "transaction restore filesystem validation is unsupported",
    ))
}

fn ownership_failure(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}
