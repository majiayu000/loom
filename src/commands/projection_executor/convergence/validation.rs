use serde_json::json;

use crate::types::ErrorCode;

use super::{
    CommandFailure, PreparedProjectionArtifact, map_ownership_fingerprint_error, path_entry_exists,
    projection_ownership_fingerprint,
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
