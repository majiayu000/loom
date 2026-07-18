use super::*;
use crate::sha256::{Sha256, to_hex};

pub(super) const OWNERSHIP_MANIFEST: &str = ".ownership-manifest.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum OwnershipAttemptState {
    Allocated,
    Ready,
    Activated,
    Abandoned,
    Retained,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct OwnershipAttempt {
    pub(super) nonce: String,
    pub(super) destination: String,
    pub(super) candidate_path: String,
    pub(super) proof: String,
    pub(super) manifest_digest: String,
    pub(super) state: OwnershipAttemptState,
}

#[derive(Serialize)]
struct OwnershipManifest<'a> {
    schema: &'static str,
    plan_id: &'a str,
    destination: &'a str,
    proof: &'a str,
}

pub(super) fn allocate_attempt(
    destination: &Path,
    plan_id: &str,
    proof: &str,
) -> std::result::Result<OwnershipAttempt, CommandFailure> {
    if !super::ownership::owner_proof_is_valid(plan_id, proof) {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "journal owner proof is invalid",
        ));
    }
    let nonce = uuid::Uuid::new_v4().hyphenated().to_string();
    let parent = destination
        .parent()
        .ok_or_else(|| CommandFailure::new(ErrorCode::StateCorrupt, "owned path has no parent"))?;
    let name = destination.file_name().ok_or_else(|| {
        CommandFailure::new(ErrorCode::StateCorrupt, "owned path has no file name")
    })?;
    let candidate = parent.join(format!(
        ".{}.ownership-attempt-{nonce}",
        name.to_string_lossy()
    ));
    let destination = destination.display().to_string();
    let raw = manifest_raw(plan_id, &destination, proof)?;
    Ok(OwnershipAttempt {
        nonce,
        destination,
        candidate_path: candidate.display().to_string(),
        proof: proof.to_string(),
        manifest_digest: digest(raw.as_bytes()),
        state: OwnershipAttemptState::Allocated,
    })
}

pub(super) fn manifest_raw(
    plan_id: &str,
    destination: &str,
    proof: &str,
) -> std::result::Result<String, CommandFailure> {
    serde_json::to_string_pretty(&OwnershipManifest {
        schema: "loom.convergence-ownership.v1",
        plan_id,
        destination,
        proof,
    })
    .map(|raw| raw + "\n")
    .map_err(map_io)
}

pub(super) fn manifest_is_exact(attempt: &OwnershipAttempt, path: &Path) -> bool {
    let manifest = path.join(OWNERSHIP_MANIFEST);
    fs::symlink_metadata(&manifest)
        .ok()
        .is_some_and(|metadata| {
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && fs::read(&manifest)
                    .ok()
                    .is_some_and(|raw| digest(&raw) == attempt.manifest_digest)
        })
}

pub(super) fn attempt_is_well_formed(attempt: &OwnershipAttempt, plan_id: &str) -> bool {
    let nonce_valid = uuid::Uuid::parse_str(&attempt.nonce)
        .ok()
        .is_some_and(|nonce| nonce.hyphenated().to_string() == attempt.nonce);
    let destination = Path::new(&attempt.destination);
    let expected_candidate = destination.parent().and_then(|parent| {
        destination.file_name().map(|name| {
            parent.join(format!(
                ".{}.ownership-attempt-{}",
                name.to_string_lossy(),
                attempt.nonce
            ))
        })
    });
    let digest_valid = manifest_raw(plan_id, &attempt.destination, &attempt.proof)
        .ok()
        .is_some_and(|raw| digest(raw.as_bytes()) == attempt.manifest_digest);
    nonce_valid
        && super::ownership::owner_proof_is_valid(plan_id, &attempt.proof)
        && expected_candidate.as_deref() == Some(Path::new(&attempt.candidate_path))
        && digest_valid
}

fn digest(raw: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

pub(super) fn archive_rolled_back_journal(
    path: &Path,
    journal: &TransactionJournal,
) -> std::result::Result<(), CommandFailure> {
    if journal.phase != TransactionPhase::RolledBackArtifactsRetained {
        return Ok(());
    }
    let nonce = journal
        .ownership_attempts
        .first()
        .map(|attempt| attempt.nonce.as_str())
        .ok_or_else(|| CommandFailure::new(ErrorCode::StateCorrupt, "ownership ledger is empty"))?;
    let archive = path
        .parent()
        .ok_or_else(|| CommandFailure::new(ErrorCode::StateCorrupt, "journal has no parent"))?
        .join(format!("retained-{}-{nonce}.json", journal.plan_id));
    rename_no_replace_atomic(path, &archive).map_err(map_io)?;
    crate::fs_util::sync_parent_directory(&archive).map_err(map_io)
}
