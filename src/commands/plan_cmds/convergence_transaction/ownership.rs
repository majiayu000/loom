use super::*;

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
    cleanup_proof_entry(
        &staging,
        &staging.join(RESERVATION_PROOF_FILE),
        expected_proof,
        "reservation staging",
        errors,
    );
    cleanup_proof_entry(
        &reservation,
        &reservation,
        expected_proof,
        "reservation token",
        errors,
    );
}

fn cleanup_proof_entry(
    entry: &Path,
    proof_path: &Path,
    expected_proof: &str,
    label: &str,
    errors: &mut Vec<Value>,
) {
    match fs::symlink_metadata(entry) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => push_rollback_error(errors, "inspect_artifact_reservation", err),
        Ok(_) if exact_regular_file(proof_path, expected_proof) => {
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
