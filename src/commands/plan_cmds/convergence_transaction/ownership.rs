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
