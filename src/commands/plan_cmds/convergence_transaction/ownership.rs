use super::ownership_state::{
    OWNERSHIP_MANIFEST, OwnershipAttemptState, allocate_attempt, attempt_is_well_formed,
    manifest_is_exact, manifest_raw,
};
use super::*;
use crate::fs_util::{paths_share_filesystem, sync_directory, sync_parent_directory};
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
    if !paths_share_filesystem(owner, live_parent).map_err(map_io)? {
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
        Ok(_) if owner_dir_is_exact(path, plan_id, owner_proof) => {}
        Ok(_) => push_rollback_error(
            errors,
            "validate_owned_transaction_artifact",
            "present transaction artifact does not match its journal ownership proof",
        ),
    }
}

pub(super) fn activate_owned_dir(
    journal_path: &Path,
    journal: &mut TransactionJournal,
    destination: &Path,
    proof: &str,
) -> std::result::Result<(), CommandFailure> {
    loop {
        let index = journal
            .ownership_attempts
            .iter()
            .position(|attempt| {
                attempt.destination == destination.display().to_string()
                    && attempt.proof == proof
                    && attempt.state != OwnershipAttemptState::Abandoned
            })
            .ok_or_else(|| ownership_failure("journal ownership attempt is absent"))?;
        let state = journal.ownership_attempts[index].state;
        let candidate = PathBuf::from(&journal.ownership_attempts[index].candidate_path);
        match state {
            OwnershipAttemptState::Allocated => {
                match fs::symlink_metadata(&candidate) {
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        journal.ownership_attempts[index].state = OwnershipAttemptState::Abandoned;
                        journal.ownership_attempts.push(allocate_attempt(
                            destination,
                            &journal.plan_id,
                            proof,
                        )?);
                        save_journal(journal_path, journal)?;
                        continue;
                    }
                    Err(err) => return Err(map_io(err)),
                }
                fs::create_dir(&candidate).map_err(map_io)?;
                write_new_synced(&candidate.join(RESERVATION_PROOF_FILE), proof)?;
                maybe_skill_fault("convergence_interrupt_after_owner_root_creation")?;
                write_new_synced(&candidate.join(OWNER_FILE), &journal.plan_id)?;
                let manifest =
                    manifest_raw(&journal.plan_id, &destination.display().to_string(), proof)?;
                write_new_synced(&candidate.join(OWNERSHIP_MANIFEST), manifest.trim_end())?;
                sync_directory(&candidate).map_err(map_io)?;
                sync_parent_directory(&candidate).map_err(map_io)?;
                journal.ownership_attempts[index].state = OwnershipAttemptState::Ready;
                save_journal(journal_path, journal)?;
                maybe_skill_fault("convergence_interrupt_after_owner_marker_write")?;
                maybe_skill_fault("convergence_interrupt_after_reservation_pending_create")?;
            }
            OwnershipAttemptState::Ready => {
                let attempt = &journal.ownership_attempts[index];
                if owned_attempt_is_exact(attempt, destination, &journal.plan_id) {
                    journal.ownership_attempts[index].state = OwnershipAttemptState::Activated;
                    save_journal(journal_path, journal)?;
                    continue;
                }
                if !owned_attempt_is_exact(attempt, &candidate, &journal.plan_id) {
                    return Err(ownership_failure("ready ownership candidate is not exact"));
                }
                rename_no_replace_atomic(&candidate, destination).map_err(|err| {
                    CommandFailure::new(
                        ErrorCode::StateCorrupt,
                        format!(
                            "owned artifact activation collision at {}: {err}",
                            destination.display()
                        ),
                    )
                })?;
                sync_parent_directory(destination).map_err(map_io)?;
                journal.ownership_attempts[index].state = OwnershipAttemptState::Activated;
                save_journal(journal_path, journal)?;
            }
            OwnershipAttemptState::Activated | OwnershipAttemptState::Retained => {
                if owned_attempt_is_exact(
                    &journal.ownership_attempts[index],
                    destination,
                    &journal.plan_id,
                ) {
                    return Ok(());
                }
                return Err(ownership_failure("activated owned artifact is not exact"));
            }
            OwnershipAttemptState::Abandoned => unreachable!("abandoned attempts are excluded"),
        }
    }
}

fn owned_attempt_is_exact(
    attempt: &super::ownership_state::OwnershipAttempt,
    path: &Path,
    plan_id: &str,
) -> bool {
    owner_dir_is_exact(path, plan_id, &attempt.proof) && manifest_is_exact(attempt, path)
}

#[cfg(test)]
pub(super) fn reserve_owned_dir(
    path: &Path,
    plan_id: &str,
    proof: &str,
) -> std::result::Result<(), CommandFailure> {
    if !owner_proof_is_valid(plan_id, proof) {
        return Err(ownership_failure("journal owner proof is invalid"));
    }
    if owner_dir_is_exact(path, plan_id, proof) {
        return Ok(());
    }
    fs::create_dir(path).map_err(map_io)?;
    write_new_synced(&path.join(RESERVATION_PROOF_FILE), proof)?;
    write_new_synced(&path.join(OWNER_FILE), plan_id)?;
    Ok(())
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

pub(super) fn validate_transaction_artifacts(journal: &TransactionJournal) -> Vec<Value> {
    let mut errors = Vec::new();
    for attempt in &journal.ownership_attempts {
        let candidate = Path::new(&attempt.candidate_path);
        let destination = Path::new(&attempt.destination);
        let valid = match attempt.state {
            OwnershipAttemptState::Allocated | OwnershipAttemptState::Abandoned => true,
            OwnershipAttemptState::Ready => {
                owned_attempt_is_exact(attempt, candidate, &journal.plan_id)
                    || owned_attempt_is_exact(attempt, destination, &journal.plan_id)
            }
            OwnershipAttemptState::Activated | OwnershipAttemptState::Retained => {
                owned_attempt_is_exact(attempt, destination, &journal.plan_id)
            }
        };
        if !valid {
            push_rollback_error(
                &mut errors,
                "validate_owned_transaction_artifact",
                format!("ownership attempt at {} is not exact", attempt.destination),
            );
        }
    }
    errors
}

pub(super) fn retain_declared_attempts(journal: &mut TransactionJournal) -> Vec<Value> {
    let mut errors = Vec::new();
    for attempt in &mut journal.ownership_attempts {
        let candidate = Path::new(&attempt.candidate_path);
        let destination = Path::new(&attempt.destination);
        attempt.state = match attempt.state {
            OwnershipAttemptState::Activated => {
                if !owned_attempt_is_exact(attempt, destination, &journal.plan_id) {
                    push_rollback_error(
                        &mut errors,
                        "retain_activated_ownership_attempt",
                        format!(
                            "activated ownership path is not exact: {}",
                            destination.display()
                        ),
                    );
                }
                OwnershipAttemptState::Retained
            }
            OwnershipAttemptState::Ready => {
                if owned_attempt_is_exact(attempt, destination, &journal.plan_id) {
                    OwnershipAttemptState::Retained
                } else if owned_attempt_is_exact(attempt, candidate, &journal.plan_id) {
                    OwnershipAttemptState::Abandoned
                } else {
                    push_rollback_error(
                        &mut errors,
                        "retain_ready_ownership_attempt",
                        "ready ownership proof is neither candidate nor destination",
                    );
                    OwnershipAttemptState::Ready
                }
            }
            OwnershipAttemptState::Allocated => OwnershipAttemptState::Abandoned,
            state => state,
        };
    }
    errors
}

pub(super) fn ownership_attempts_match_journal(journal: &TransactionJournal) -> bool {
    let terminal_preparation_rollback = is_pre_mutation_retained(journal);
    let mut expected = vec![(
        journal.artifact_root.as_str(),
        journal.artifact_owner_proof.as_str(),
    )];
    if let (Some(staging), Some(proof)) = (
        journal.source_staging.as_deref(),
        journal.source_owner_proof.as_deref(),
    ) && let Some(owner) = Path::new(staging).parent().and_then(Path::to_str)
    {
        expected.push((owner, proof));
    }
    expected.extend(journal.projections.iter().map(|projection| {
        (
            projection.staging_owner.as_str(),
            projection.owner_proof.as_str(),
        )
    }));
    expected.iter().all(|(destination, proof)| {
        let matching = journal
            .ownership_attempts
            .iter()
            .filter(|attempt| attempt.destination == *destination && attempt.proof == *proof);
        if terminal_preparation_rollback {
            matching.clone().next().is_some()
                && matching.clone().all(|attempt| {
                    matches!(
                        attempt.state,
                        OwnershipAttemptState::Abandoned | OwnershipAttemptState::Retained
                    )
                })
                && matching
                    .filter(|attempt| attempt.state == OwnershipAttemptState::Retained)
                    .count()
                    <= 1
        } else {
            matching
                .filter(|attempt| attempt.state != OwnershipAttemptState::Abandoned)
                .count()
                == 1
        }
    }) && journal.ownership_attempts.iter().all(|attempt| {
        (matches!(
            attempt.state,
            OwnershipAttemptState::Abandoned | OwnershipAttemptState::Retained
        ) || expected.iter().any(|(destination, proof)| {
            attempt.destination == *destination && attempt.proof == *proof
        })) && attempt_is_well_formed(attempt, &journal.plan_id)
    })
}

pub(super) fn is_pre_mutation_retained(journal: &TransactionJournal) -> bool {
    journal.preparation_aborted
        && journal.phase == TransactionPhase::RolledBackArtifactsRetained
        && journal.source_head.is_none()
        && journal.source_commit.is_none()
        && journal.source_staged_index_digest.is_none()
        && journal.source_index_changed.is_none()
        && journal.expected_projections.is_none()
        && journal.registry_commit.is_none()
        && journal.registry_staged_index_digest.is_none()
        && journal.registry_index_attempts.is_empty()
        && journal.result.is_none()
        && journal.installed_projections == 0
        && journal
            .projections
            .iter()
            .all(|projection| !projection.is_activated())
}

fn ownership_failure(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}
