use super::*;

const OWNER_FILE: &str = ".owner";
const RESERVATION_PROOF_FILE: &str = ".reservation-owner";

pub(super) fn validate_owned_staging(
    live: &Path,
    staging: &Path,
    plan_id: &str,
) -> std::result::Result<(), CommandFailure> {
    let owner = staging
        .parent()
        .ok_or_else(|| ownership_failure("transaction staging has no owner directory"))?;
    let owner_metadata = fs::symlink_metadata(owner).map_err(map_io)?;
    if owner_metadata.file_type().is_symlink() || !owner_metadata.is_dir() {
        return Err(ownership_failure(
            "transaction staging owner is not a real directory",
        ));
    }
    let owner_marker = owner.join(OWNER_FILE);
    let marker_metadata = fs::symlink_metadata(&owner_marker).map_err(map_io)?;
    if marker_metadata.file_type().is_symlink() || !marker_metadata.is_file() {
        return Err(ownership_failure(
            "transaction staging owner marker is not a real file",
        ));
    }
    if fs::read_to_string(&owner_marker).map_err(map_io)?.trim() != plan_id {
        return Err(ownership_failure(
            "transaction staging owner marker does not match the plan",
        ));
    }
    let proof_path = owner.join(RESERVATION_PROOF_FILE);
    let proof_metadata = fs::symlink_metadata(&proof_path).map_err(map_io)?;
    if proof_metadata.file_type().is_symlink() || !proof_metadata.is_file() {
        return Err(ownership_failure(
            "transaction staging reservation proof is not a real file",
        ));
    }
    let proof = fs::read_to_string(proof_path).map_err(map_io)?;
    let nonce = proof
        .trim()
        .strip_prefix(&format!("{plan_id}:"))
        .ok_or_else(|| {
            ownership_failure("transaction staging reservation proof has the wrong owner")
        })?;
    if uuid::Uuid::parse_str(nonce)
        .ok()
        .is_none_or(|parsed| parsed.hyphenated().to_string() != nonce)
    {
        return Err(ownership_failure(
            "transaction staging reservation proof is invalid",
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
