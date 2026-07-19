use serde_json::json;

use crate::fs_util::{exchange_paths_atomic, rename_no_replace_atomic};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::{
    CommandFailure, PreparedProjection, ProjectionActivationOutput, ProjectionRollbackArtifact,
    apply_projection_observation, cleanup_prepare_failure, map_atomic_exchange_error,
    map_atomic_operation_error, map_io, observe_projection_from_source, path_entry_exists,
    projection_ownership_fingerprint, with_prepared_recovery_details,
};

#[allow(
    dead_code,
    reason = "consumed by the SP524-T004 convergence transaction"
)]
pub(crate) fn activate_prepared_projection(
    _ctx: &AppContext,
    prepared: PreparedProjection,
) -> Result<ProjectionActivationOutput, CommandFailure> {
    #[cfg(debug_assertions)]
    let staging_path = prepared
        .durable_artifact()
        .staging_path
        .display()
        .to_string();
    activate_prepared_projection_with_after_mutation(prepared, || {
        #[cfg(debug_assertions)]
        let selected = std::env::var("LOOM_TEST_CONVERGENCE_ACTIVATION_STAGING")
            .ok()
            .is_none_or(|value| value == staging_path);
        #[cfg(not(debug_assertions))]
        let selected = true;
        if std::env::var("LOOM_FAULT_INJECT").ok().as_deref()
            == Some("convergence_interrupt_after_projection_activation")
            && selected
        {
            return Err(std::io::Error::other(
                "fault injection: convergence_interrupt_after_projection_activation",
            ));
        }
        Ok(())
    })
}

fn activate_prepared_projection_with_after_mutation(
    prepared: PreparedProjection,
    after_mutation: impl Fn() -> std::io::Result<()>,
) -> Result<ProjectionActivationOutput, CommandFailure> {
    let mut prepared = prepared;
    let parts = prepared.take_parts();
    let mut rollback_artifact = if parts.path_exists {
        let staging_digest = observed_digest(&parts.staging_path, &parts)?;
        let live_digest = observed_digest(&parts.materialized_path, &parts)?;
        let original_digest = parts
            .existing_digest
            .as_ref()
            .expect("existing convergence path must have a prepared digest");
        if staging_digest.as_deref() == Some(&parts.staging_digest)
            && live_digest.as_deref() == Some(original_digest)
        {
            if let Err(err) = exchange_paths_atomic(&parts.staging_path, &parts.materialized_path) {
                return Err(cleanup_prepare_failure(
                    map_atomic_exchange_error(err),
                    &parts,
                ));
            }
            after_mutation()
                .map_err(map_io)
                .map_err(|err| with_prepared_recovery_details(err, &parts))?;
        } else if staging_digest.as_deref() == Some(&parts.staging_digest) {
            return Err(cleanup_prepare_failure(
                activation_state_mismatch(&parts, staging_digest, live_digest),
                &parts,
            ));
        } else if staging_digest.as_deref() != Some(original_digest)
            || live_digest.as_deref() != Some(&parts.staging_digest)
        {
            return Err(activation_state_mismatch(
                &parts,
                staging_digest,
                live_digest,
            ));
        }
        ProjectionRollbackArtifact::Exchanged {
            materialized_path: parts.materialized_path,
            backup_path: parts.staging_path,
            activated_digest: parts.staging_digest,
            original_digest: original_digest.clone(),
        }
    } else {
        let staging_digest = observed_digest(&parts.staging_path, &parts)?;
        let live_digest = observed_digest(&parts.materialized_path, &parts)?;
        if staging_digest.as_deref() == Some(&parts.staging_digest) && live_digest.is_none() {
            if let Err(err) =
                rename_no_replace_atomic(&parts.staging_path, &parts.materialized_path)
            {
                return Err(cleanup_prepare_failure(
                    map_atomic_operation_error(err, &parts.materialized_path),
                    &parts,
                ));
            }
            after_mutation()
                .map_err(map_io)
                .map_err(|err| with_prepared_recovery_details(err, &parts))?;
        } else if staging_digest.as_deref() == Some(&parts.staging_digest) {
            return Err(cleanup_prepare_failure(
                activation_state_mismatch(&parts, staging_digest, live_digest),
                &parts,
            ));
        } else if staging_digest.is_some() || live_digest.as_deref() != Some(&parts.staging_digest)
        {
            return Err(activation_state_mismatch(
                &parts,
                staging_digest,
                live_digest,
            ));
        }
        ProjectionRollbackArtifact::Created {
            materialized_path: parts.materialized_path,
            rollback_path: parts.staging_path,
            activated_digest: parts.staging_digest,
        }
    };

    let mut projection = parts.projection;
    let observation = observe_projection_from_source(&projection, &parts.source_path);
    if observation.status != "healthy" {
        let mut rollback_errors = Vec::new();
        if let Err(err) = rollback_artifact.rollback() {
            rollback_errors.push(json!({
                "step": "rollback_projection_activation",
                "code": err.code.as_str(),
                "message": err.message,
                "details": err.details,
                "artifact": rollback_artifact.evidence(),
            }));
        }
        return Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "convergence projection '{}' failed post-activation validation: {}",
                projection.instance_id,
                observation
                    .error_code
                    .unwrap_or("projection_validation_failed")
            ),
        )
        .with_rollback_errors(rollback_errors));
    }
    apply_projection_observation(&mut projection, &observation);
    Ok(ProjectionActivationOutput {
        projection: Some(projection),
        rollback_artifact: Some(rollback_artifact),
        #[cfg(test)]
        fail_cleanup_once: false,
    })
}

#[cfg(test)]
pub(crate) fn activate_after_mutation(
    prepared: PreparedProjection,
    after_mutation: impl Fn() -> std::io::Result<()>,
) -> Result<ProjectionActivationOutput, CommandFailure> {
    activate_prepared_projection_with_after_mutation(prepared, after_mutation)
}

fn observed_digest(
    path: &std::path::Path,
    parts: &super::PreparedProjectionArtifact,
) -> Result<Option<String>, CommandFailure> {
    if !path_entry_exists(path)
        .map_err(map_io)
        .map_err(|err| with_prepared_recovery_details(err, parts))?
    {
        return Ok(None);
    }
    projection_ownership_fingerprint(path)
        .map(Some)
        .map_err(|err| super::map_ownership_fingerprint_error(err, path))
        .map_err(|err| with_prepared_recovery_details(err, parts))
}

fn activation_state_mismatch(
    parts: &super::PreparedProjectionArtifact,
    staging_digest: Option<String>,
    live_digest: Option<String>,
) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        "cannot identify a safe projection activation state; concurrent data was preserved",
    );
    failure.details = json!({
        "staging_digest": staging_digest,
        "live_digest": live_digest,
    });
    with_prepared_recovery_details(failure, parts)
}

#[allow(
    dead_code,
    reason = "consumed by the SP524-T004 convergence transaction"
)]
pub(crate) fn discard_prepared_projection(
    mut prepared: PreparedProjection,
) -> Result<(), CommandFailure> {
    let parts = prepared.take_parts();
    super::cleanup_prepared_artifact(&parts)
}
