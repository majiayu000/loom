use serde_json::json;

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
    let scope = prepared.take_scope(&parts)?;
    validate_scope_binding(&scope, &parts)?;
    let rollback_artifact = if parts.path_exists {
        let staging_digest = observed_digest(&parts.staging_path, &parts)?;
        let live_digest = observed_digest(&parts.materialized_path, &parts)?;
        validate_scope_binding(&scope, &parts)?;
        let original_digest = parts
            .existing_digest
            .as_ref()
            .expect("existing convergence path must have a prepared digest");
        if staging_digest.as_deref() == Some(&parts.staging_digest)
            && live_digest.as_deref() == Some(original_digest)
        {
            let rollback_artifact = ProjectionRollbackArtifact::Exchanged {
                materialized_path: parts.materialized_path.clone(),
                backup_path: parts.staging_path.clone(),
                activated_digest: parts.staging_digest.clone(),
                original_digest: original_digest.clone(),
            };
            if let Err(err) = scope.exchange() {
                return Err(with_prepared_recovery_details(
                    map_atomic_exchange_error(err),
                    &parts,
                ));
            }
            if let Err(error) = after_mutation()
                .map_err(map_io)
                .map_err(|err| with_prepared_recovery_details(err, &parts))
                .and_then(|_| validate_scope_binding(&scope, &parts))
            {
                return Err(rollback_after_mutation_failure(
                    &scope,
                    &rollback_artifact,
                    error,
                ));
            }
            rollback_artifact
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
        } else {
            ProjectionRollbackArtifact::Exchanged {
                materialized_path: parts.materialized_path.clone(),
                backup_path: parts.staging_path.clone(),
                activated_digest: parts.staging_digest.clone(),
                original_digest: original_digest.clone(),
            }
        }
    } else {
        let staging_digest = observed_digest(&parts.staging_path, &parts)?;
        let live_digest = observed_digest(&parts.materialized_path, &parts)?;
        validate_scope_binding(&scope, &parts)?;
        if staging_digest.as_deref() == Some(&parts.staging_digest) && live_digest.is_none() {
            let rollback_artifact = ProjectionRollbackArtifact::Created {
                materialized_path: parts.materialized_path.clone(),
                rollback_path: parts.staging_path.clone(),
                activated_digest: parts.staging_digest.clone(),
            };
            if let Err(err) = scope.activate_create() {
                return Err(with_prepared_recovery_details(
                    map_atomic_operation_error(err, &parts.materialized_path),
                    &parts,
                ));
            }
            if let Err(error) = after_mutation()
                .map_err(map_io)
                .map_err(|err| with_prepared_recovery_details(err, &parts))
                .and_then(|_| validate_scope_binding(&scope, &parts))
            {
                return Err(rollback_after_mutation_failure(
                    &scope,
                    &rollback_artifact,
                    error,
                ));
            }
            rollback_artifact
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
        } else {
            ProjectionRollbackArtifact::Created {
                materialized_path: parts.materialized_path.clone(),
                rollback_path: parts.staging_path.clone(),
                activated_digest: parts.staging_digest.clone(),
            }
        }
    };

    validate_scope_binding(&scope, &parts)?;
    let observation = observe_projection_from_source(&parts.projection, &parts.source_path);
    validate_scope_binding(&scope, &parts)?;
    let mut projection = parts.projection;
    if observation.status != "healthy" {
        let mut rollback_errors = Vec::new();
        let rollback = match rollback_artifact {
            ProjectionRollbackArtifact::Exchanged { .. } => scope.exchange(),
            ProjectionRollbackArtifact::Created { .. } => scope.rollback_create(),
            ProjectionRollbackArtifact::PendingCleanup { .. } => Err(std::io::Error::other(
                "new activation unexpectedly has pending cleanup evidence",
            )),
        };
        if let Err(err) = rollback {
            rollback_errors.push(json!({
                "step": "rollback_projection_activation",
                "code": ErrorCode::IoError.as_str(),
                "message": err.to_string(),
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
        scope: Some(scope),
        #[cfg(test)]
        fail_cleanup_once: false,
    })
}

fn rollback_after_mutation_failure(
    scope: &super::PreparedProjectionScope,
    artifact: &ProjectionRollbackArtifact,
    failure: CommandFailure,
) -> CommandFailure {
    let rollback = match artifact {
        ProjectionRollbackArtifact::Exchanged { .. } => scope.exchange(),
        ProjectionRollbackArtifact::Created { .. } => scope.rollback_create(),
        ProjectionRollbackArtifact::PendingCleanup { .. } => Err(std::io::Error::other(
            "new activation unexpectedly has pending cleanup evidence",
        )),
    };
    match rollback {
        Ok(()) => failure,
        Err(error) => failure.with_rollback_errors(vec![json!({
            "step": "rollback_projection_after_post_mutation_failure",
            "code": ErrorCode::IoError.as_str(),
            "message": error.to_string(),
            "artifact": artifact.evidence(),
        })]),
    }
}

fn validate_scope_binding(
    scope: &super::PreparedProjectionScope,
    parts: &super::PreparedProjectionArtifact,
) -> Result<(), CommandFailure> {
    scope
        .validate_path_binding()
        .map_err(|error| with_prepared_recovery_details(error, parts))
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
