use serde_json::json;

use crate::types::ErrorCode;

use super::{
    CommandFailure, PendingCleanupReason, PreparedProjectionArtifact, ProjectionRollbackArtifact,
    map_ownership_fingerprint_error, owned_digest, path_entry_exists,
    projection_ownership_fingerprint, with_recovery_details,
};

pub(crate) fn validate_prepared_projection_artifact(
    artifact: &PreparedProjectionArtifact,
) -> Result<(), CommandFailure> {
    let staging_exists = path_entry_exists(&artifact.staging_path).map_err(super::map_io)?;
    let staging_digest = if staging_exists {
        Some(
            projection_ownership_fingerprint(&artifact.staging_path)
                .map_err(|err| map_ownership_fingerprint_error(err, &artifact.staging_path))?,
        )
    } else {
        None
    };
    let live_exists = path_entry_exists(&artifact.materialized_path).map_err(super::map_io)?;
    let live_digest = if live_exists {
        Some(
            projection_ownership_fingerprint(&artifact.materialized_path)
                .map_err(|err| map_ownership_fingerprint_error(err, &artifact.materialized_path))?,
        )
    } else {
        None
    };
    let before_activation = staging_digest.as_deref() == Some(&artifact.staging_digest)
        && live_exists == artifact.path_exists
        && live_digest == artifact.existing_digest;
    let after_activation = live_digest.as_deref() == Some(&artifact.staging_digest)
        && if artifact.path_exists {
            artifact.existing_digest.as_ref() == staging_digest.as_ref()
        } else {
            staging_digest.is_none()
        };
    if !before_activation && !after_activation {
        return Err(mismatch(
            artifact,
            live_digest,
            "projection paths changed outside the prepared activation",
        ));
    }
    Ok(())
}

pub(crate) fn validate_projection_rollback_artifact_for_finalize(
    artifact: &ProjectionRollbackArtifact,
) -> Result<(), CommandFailure> {
    match artifact {
        ProjectionRollbackArtifact::Exchanged {
            materialized_path,
            backup_path,
            activated_digest,
            original_digest,
        } => {
            validate_distinct_paths(materialized_path, backup_path, artifact)?;
            let claim_path = sibling_claim(backup_path, ".finalize-claim", artifact)?;
            let live = owned_digest(materialized_path, artifact)?;
            let backup = owned_digest(backup_path, artifact)?;
            let claim = owned_digest(&claim_path, artifact)?;
            let valid = live.as_deref() == Some(activated_digest)
                && ((backup.as_deref() == Some(original_digest) && claim.is_none())
                    || (backup.is_none() && claim.as_deref() == Some(original_digest)));
            validate_artifact_state(valid, artifact, live, backup, claim, "finalize")
        }
        ProjectionRollbackArtifact::Created {
            materialized_path,
            rollback_path,
            activated_digest,
        } => {
            validate_distinct_paths(materialized_path, rollback_path, artifact)?;
            let live = owned_digest(materialized_path, artifact)?;
            let rollback = owned_digest(rollback_path, artifact)?;
            validate_artifact_state(
                live.as_deref() == Some(activated_digest) && rollback.is_none(),
                artifact,
                live,
                rollback,
                None,
                "finalize",
            )
        }
        ProjectionRollbackArtifact::PendingCleanup { reason, .. } => {
            if *reason != PendingCleanupReason::FinalizeExchanged {
                return Err(super::invalid_recovery_transition("finalize", artifact));
            }
            validate_pending_cleanup(artifact, true)
        }
    }
}

pub(crate) fn validate_projection_rollback_artifact_for_rollback(
    artifact: &ProjectionRollbackArtifact,
) -> Result<(), CommandFailure> {
    match artifact {
        ProjectionRollbackArtifact::Exchanged {
            materialized_path,
            backup_path,
            activated_digest,
            original_digest,
        } => {
            validate_distinct_paths(materialized_path, backup_path, artifact)?;
            let live = owned_digest(materialized_path, artifact)?;
            let backup = owned_digest(backup_path, artifact)?;
            let valid = (live.as_deref() == Some(activated_digest)
                && backup.as_deref() == Some(original_digest))
                || (live.as_deref() == Some(original_digest)
                    && backup.as_deref() == Some(activated_digest));
            validate_artifact_state(valid, artifact, live, backup, None, "rollback")
        }
        ProjectionRollbackArtifact::Created {
            materialized_path,
            rollback_path,
            activated_digest,
        } => {
            validate_distinct_paths(materialized_path, rollback_path, artifact)?;
            let live = owned_digest(materialized_path, artifact)?;
            let rollback = owned_digest(rollback_path, artifact)?;
            let valid = (live.as_deref() == Some(activated_digest) && rollback.is_none())
                || (live.is_none() && rollback.as_deref() == Some(activated_digest));
            validate_artifact_state(valid, artifact, live, rollback, None, "rollback")
        }
        ProjectionRollbackArtifact::PendingCleanup { reason, .. } => {
            if *reason == PendingCleanupReason::FinalizeExchanged {
                return Err(super::invalid_recovery_transition("rollback", artifact));
            }
            validate_pending_cleanup(artifact, false)
        }
    }
}

fn validate_pending_cleanup(
    artifact: &ProjectionRollbackArtifact,
    finalize: bool,
) -> Result<(), CommandFailure> {
    let ProjectionRollbackArtifact::PendingCleanup {
        materialized_path,
        artifact_path,
        expected_live_digest,
        expected_digest,
        reason: _,
    } = artifact
    else {
        unreachable!("pending cleanup validator requires pending cleanup evidence");
    };
    validate_distinct_paths(materialized_path, artifact_path, artifact)?;
    let claim_path = sibling_claim(artifact_path, ".pending-cleanup-claim", artifact)?;
    let live = owned_digest(materialized_path, artifact)?;
    let pending = owned_digest(artifact_path, artifact)?;
    let claim = owned_digest(&claim_path, artifact)?;
    let artifact_valid = (pending.as_deref() == Some(expected_digest) && claim.is_none())
        || (pending.is_none() && (claim.as_deref() == Some(expected_digest) || claim.is_none()));
    let live_valid = live.as_ref() == expected_live_digest.as_ref();
    validate_artifact_state(
        artifact_valid && live_valid,
        artifact,
        live,
        pending,
        claim,
        if finalize { "finalize" } else { "rollback" },
    )
}

fn validate_distinct_paths(
    materialized_path: &std::path::Path,
    artifact_path: &std::path::Path,
    artifact: &ProjectionRollbackArtifact,
) -> Result<(), CommandFailure> {
    if materialized_path == artifact_path
        || materialized_path.parent().is_none()
        || materialized_path.parent() != artifact_path.parent()
    {
        return Err(with_recovery_details(
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "projection rollback artifact has an invalid path layout",
            ),
            artifact,
        ));
    }
    Ok(())
}

fn sibling_claim(
    path: &std::path::Path,
    suffix: &str,
    artifact: &ProjectionRollbackArtifact,
) -> Result<std::path::PathBuf, CommandFailure> {
    let name = path.file_name().ok_or_else(|| {
        with_recovery_details(
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "projection artifact path has no file name",
            ),
            artifact,
        )
    })?;
    Ok(path.with_file_name(format!("{}{suffix}", name.to_string_lossy())))
}

fn validate_artifact_state(
    valid: bool,
    artifact: &ProjectionRollbackArtifact,
    live_digest: Option<String>,
    artifact_digest: Option<String>,
    claim_digest: Option<String>,
    operation: &str,
) -> Result<(), CommandFailure> {
    if valid {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!("cannot {operation} projection because its durable rollback evidence changed"),
    );
    failure.details = json!({
        "live_digest": live_digest,
        "artifact_digest": artifact_digest,
        "claim_digest": claim_digest,
    });
    Err(with_recovery_details(failure, artifact))
}

fn mismatch(
    artifact: &PreparedProjectionArtifact,
    actual_digest: Option<String>,
    message: &str,
) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::ProjectionConflict, message);
    failure.details = json!({
        "materialized_path": artifact.materialized_path,
        "staging_path": artifact.staging_path,
        "actual_digest": actual_digest,
        "prepared_staging_digest": artifact.staging_digest,
        "prepared_existing_digest": artifact.existing_digest,
    });
    failure
}
