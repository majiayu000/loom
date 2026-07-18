use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::fs_util::{exchange_paths_atomic, remove_path_if_exists, rename_no_replace_atomic};
use crate::state::AppContext;
use crate::state_model::RegistryProjectionInstance;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::super::projections::{apply_projection_observation, observe_projection};
use super::super::skill_cmds::shared::push_rollback_error;

mod ownership;
pub(super) use ownership::{map_ownership_fingerprint_error, projection_ownership_fingerprint};

struct PreparedProjectionParts {
    projection: RegistryProjectionInstance,
    staging_path: PathBuf,
    materialized_path: PathBuf,
    path_exists: bool,
    staging_digest: String,
    existing_digest: Option<String>,
}

#[must_use = "a prepared projection must be activated or explicitly discarded"]
pub(crate) struct PreparedProjection {
    parts: Option<PreparedProjectionParts>,
}

impl PreparedProjection {
    pub(super) fn new(
        projection: RegistryProjectionInstance,
        staging_path: PathBuf,
        materialized_path: PathBuf,
        path_exists: bool,
        staging_digest: String,
        existing_digest: Option<String>,
    ) -> Self {
        Self {
            parts: Some(PreparedProjectionParts {
                projection,
                staging_path,
                materialized_path,
                path_exists,
                staging_digest,
                existing_digest,
            }),
        }
    }

    #[cfg(test)]
    pub(super) fn staging_path(&self) -> &Path {
        &self
            .parts
            .as_ref()
            .expect("prepared projection must own its transaction state")
            .staging_path
    }

    fn take_parts(&mut self) -> PreparedProjectionParts {
        self.parts
            .take()
            .expect("prepared projection must own its transaction state")
    }
}

impl Drop for PreparedProjection {
    fn drop(&mut self) {
        if let Some(parts) = self.parts.take()
            && let Err(err) = remove_path_if_exists(&parts.staging_path)
        {
            eprintln!(
                "loom: failed to clean abandoned prepared projection '{}': {err}",
                parts.staging_path.display()
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingCleanupReason {
    RollbackExchanged,
    RollbackCreated,
}

impl PendingCleanupReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::RollbackExchanged => "rollback_exchanged",
            Self::RollbackCreated => "rollback_created",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionRollbackArtifact {
    Exchanged {
        materialized_path: PathBuf,
        backup_path: PathBuf,
        activated_digest: String,
        original_digest: String,
    },
    Created {
        materialized_path: PathBuf,
        rollback_path: PathBuf,
        activated_digest: String,
    },
    PendingCleanup {
        materialized_path: PathBuf,
        artifact_path: PathBuf,
        expected_digest: String,
        reason: PendingCleanupReason,
    },
}

impl ProjectionRollbackArtifact {
    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn evidence(&self) -> Value {
        match self {
            Self::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => json!({
                "reason": "convergence.atomic_exchange",
                "kind": "atomic_exchange",
                "original_path": materialized_path.display().to_string(),
                "backup_path": backup_path.display().to_string(),
                "activated_digest": activated_digest,
                "original_digest": original_digest,
            }),
            Self::Created {
                materialized_path,
                rollback_path,
                activated_digest,
            } => json!({
                "reason": "convergence.atomic_create",
                "kind": "atomic_create",
                "original_path": materialized_path.display().to_string(),
                "rollback_path": rollback_path.display().to_string(),
                "activated_digest": activated_digest,
            }),
            Self::PendingCleanup {
                materialized_path,
                artifact_path,
                expected_digest,
                reason,
            } => json!({
                "reason": reason.as_str(),
                "kind": "pending_cleanup",
                "original_path": materialized_path.display().to_string(),
                "artifact_path": artifact_path.display().to_string(),
                "expected_digest": expected_digest,
            }),
        }
    }

    fn rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        self.begin_rollback()?;
        self.cleanup_pending()
    }

    fn begin_rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        match self.clone() {
            Self::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => {
                validate_owned_digest(
                    &materialized_path,
                    &activated_digest,
                    "rollback live projection",
                    self,
                )?;
                validate_owned_digest(
                    &backup_path,
                    &original_digest,
                    "rollback projection backup",
                    self,
                )?;
                exchange_paths_atomic(&backup_path, &materialized_path)
                    .map_err(map_atomic_exchange_error)
                    .map_err(|err| with_recovery_details(err, self))?;
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: backup_path,
                    expected_digest: activated_digest,
                    reason: PendingCleanupReason::RollbackExchanged,
                };
                Ok(())
            }
            Self::Created {
                materialized_path,
                rollback_path,
                activated_digest,
            } => {
                validate_owned_digest(
                    &materialized_path,
                    &activated_digest,
                    "rollback created projection",
                    self,
                )?;
                rename_no_replace_atomic(&materialized_path, &rollback_path)
                    .map_err(|err| map_atomic_operation_error(err, &rollback_path))
                    .map_err(|err| with_recovery_details(err, self))?;
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: rollback_path,
                    expected_digest: activated_digest,
                    reason: PendingCleanupReason::RollbackCreated,
                };
                Ok(())
            }
            Self::PendingCleanup { .. } => Ok(()),
        }
    }

    fn rollback_took_effect(&self) -> bool {
        matches!(self, Self::PendingCleanup { .. })
    }

    fn finalize(&mut self) -> std::result::Result<(), CommandFailure> {
        match self.clone() {
            Self::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => {
                validate_owned_digest(
                    &materialized_path,
                    &activated_digest,
                    "finalize live projection",
                    self,
                )?;
                validate_owned_digest(
                    &backup_path,
                    &original_digest,
                    "finalize projection backup",
                    self,
                )?;
                remove_path_if_exists(&backup_path)
                    .map_err(map_io)
                    .map_err(|err| with_recovery_details(err, self))
            }
            Self::Created {
                materialized_path,
                activated_digest,
                ..
            } => validate_owned_digest(
                &materialized_path,
                &activated_digest,
                "finalize live projection",
                self,
            ),
            Self::PendingCleanup { .. } => Err(with_recovery_details(
                CommandFailure::new(
                    ErrorCode::ProjectionConflict,
                    "cannot finalize after rollback took effect; retry rollback cleanup",
                ),
                self,
            )),
        }
    }

    fn cleanup_pending(&mut self) -> std::result::Result<(), CommandFailure> {
        let (artifact_path, expected_digest) = match self {
            Self::PendingCleanup {
                artifact_path,
                expected_digest,
                ..
            } => (artifact_path.clone(), expected_digest.clone()),
            _ => {
                return Err(CommandFailure::new(
                    ErrorCode::InternalError,
                    "projection rollback artifact is not pending cleanup",
                ));
            }
        };
        if !artifact_path
            .try_exists()
            .map_err(map_io)
            .map_err(|err| with_recovery_details(err, self))?
        {
            return Ok(());
        }
        if let Err(validation_error) = validate_owned_digest(
            &artifact_path,
            &expected_digest,
            "clean projection rollback artifact",
            self,
        ) {
            if !artifact_path
                .try_exists()
                .map_err(map_io)
                .map_err(|err| with_recovery_details(err, self))?
            {
                return Ok(());
            }
            return Err(validation_error);
        }
        remove_path_if_exists(&artifact_path)
            .map_err(map_io)
            .map_err(|err| with_recovery_details(err, self))
    }
}

#[must_use = "an activated projection must be finalized or rolled back"]
pub(crate) struct ProjectionActivationOutput {
    projection: Option<RegistryProjectionInstance>,
    rollback_artifact: Option<ProjectionRollbackArtifact>,
    #[cfg(test)]
    fail_cleanup_once: bool,
}

impl ProjectionActivationOutput {
    #[cfg(test)]
    pub(crate) fn projection(&self) -> &RegistryProjectionInstance {
        self.projection
            .as_ref()
            .expect("activated projection must own its projection identity")
    }

    #[cfg(test)]
    pub(crate) fn rollback_evidence(&self) -> Value {
        self.rollback_artifact
            .as_ref()
            .expect("activated projection must own a rollback artifact")
            .evidence()
    }

    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        let artifact = self.rollback_artifact.as_mut().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "activated projection has no rollback artifact",
            )
        })?;
        let transition = artifact.begin_rollback();
        if artifact.rollback_took_effect() {
            self.projection = None;
        }
        transition?;
        #[cfg(test)]
        if self.fail_cleanup_once {
            self.fail_cleanup_once = false;
            return Err(with_recovery_details(
                CommandFailure::new(
                    ErrorCode::InternalError,
                    "fault injected before rollback artifact cleanup",
                ),
                artifact,
            ));
        }
        artifact.cleanup_pending()?;
        self.rollback_artifact = None;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fail_cleanup_once_for_test(&mut self) {
        self.fail_cleanup_once = true;
    }

    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn finalize(
        &mut self,
    ) -> std::result::Result<RegistryProjectionInstance, CommandFailure> {
        let artifact = self
            .rollback_artifact
            .as_mut()
            .expect("activated projection must own a rollback artifact");
        artifact.finalize()?;
        self.rollback_artifact = None;
        Ok(self
            .projection
            .take()
            .expect("activated projection must own its projection identity"))
    }
}

impl Drop for ProjectionActivationOutput {
    fn drop(&mut self) {
        if let Some(artifact) = self.rollback_artifact.as_mut()
            && let Err(err) = artifact.rollback()
        {
            eprintln!(
                "loom: abandoned projection activation requires recovery: {}; details={}",
                err.message, err.details
            );
        }
    }
}

#[allow(
    dead_code,
    reason = "consumed by the SP524-T004 convergence transaction"
)]
pub(crate) fn activate_prepared_projection(
    ctx: &AppContext,
    prepared: PreparedProjection,
) -> std::result::Result<ProjectionActivationOutput, CommandFailure> {
    let mut prepared = prepared;
    let parts = prepared.take_parts();
    validate_before_activation(&parts).map_err(|err| cleanup_prepare_failure(err, &parts))?;

    let mut rollback_artifact = if parts.path_exists {
        if let Err(err) = exchange_paths_atomic(&parts.staging_path, &parts.materialized_path) {
            return Err(cleanup_prepare_failure(
                map_atomic_exchange_error(err),
                &parts,
            ));
        }
        ProjectionRollbackArtifact::Exchanged {
            materialized_path: parts.materialized_path,
            backup_path: parts.staging_path,
            activated_digest: parts.staging_digest,
            original_digest: parts
                .existing_digest
                .expect("existing convergence path must have a prepared digest"),
        }
    } else {
        if let Err(err) = rename_no_replace_atomic(&parts.staging_path, &parts.materialized_path) {
            return Err(cleanup_prepare_failure(
                map_atomic_operation_error(err, &parts.materialized_path),
                &parts,
            ));
        }
        ProjectionRollbackArtifact::Created {
            materialized_path: parts.materialized_path,
            rollback_path: parts.staging_path,
            activated_digest: parts.staging_digest,
        }
    };

    let mut projection = parts.projection;
    let observation = observe_projection(ctx, &projection);
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

#[allow(
    dead_code,
    reason = "consumed by the SP524-T004 convergence transaction"
)]
pub(crate) fn discard_prepared_projection(
    mut prepared: PreparedProjection,
) -> std::result::Result<(), CommandFailure> {
    let parts = prepared.take_parts();
    remove_path_if_exists(&parts.staging_path).map_err(map_io)
}

fn validate_before_activation(
    parts: &PreparedProjectionParts,
) -> std::result::Result<(), CommandFailure> {
    validate_prepared_digest(
        &parts.staging_path,
        &parts.staging_digest,
        "staging projection",
    )?;
    if let Some(existing_digest) = &parts.existing_digest {
        validate_prepared_digest(
            &parts.materialized_path,
            existing_digest,
            "existing live projection",
        )?;
    }
    Ok(())
}

fn validate_prepared_digest(
    path: &Path,
    expected_digest: &str,
    label: &str,
) -> std::result::Result<(), CommandFailure> {
    let actual_digest = projection_ownership_fingerprint(path)
        .map_err(|err| map_ownership_fingerprint_error(err, path))?;
    if actual_digest == expected_digest {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!("{label} '{}' changed after preparation", path.display()),
    );
    failure.details = json!({
        "path": path.display().to_string(),
        "expected_digest": expected_digest,
        "actual_digest": actual_digest,
    });
    Err(failure)
}

fn validate_owned_digest(
    path: &Path,
    expected_digest: &str,
    operation: &str,
    artifact: &ProjectionRollbackArtifact,
) -> std::result::Result<(), CommandFailure> {
    let actual_digest = projection_ownership_fingerprint(path).map_err(|err| {
        with_recovery_details(map_ownership_fingerprint_error(err, path), artifact)
    })?;
    if actual_digest == expected_digest {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!(
            "cannot {operation} because '{}' changed after activation; concurrent data was preserved",
            path.display()
        ),
    );
    failure.details = json!({
        "path": path.display().to_string(),
        "expected_digest": expected_digest,
        "actual_digest": actual_digest,
    });
    Err(with_recovery_details(failure, artifact))
}

fn cleanup_prepare_failure(err: CommandFailure, parts: &PreparedProjectionParts) -> CommandFailure {
    let mut cleanup_errors = Vec::new();
    if let Err(cleanup_err) = remove_path_if_exists(&parts.staging_path) {
        push_rollback_error(
            &mut cleanup_errors,
            "remove_projection_staging",
            cleanup_err,
        );
    }
    err.with_rollback_errors(cleanup_errors)
}

fn with_recovery_details(
    mut failure: CommandFailure,
    artifact: &ProjectionRollbackArtifact,
) -> CommandFailure {
    let cause_details = std::mem::replace(&mut failure.details, json!({}));
    failure.details = json!({
        "recovery_required": true,
        "artifact": artifact.evidence(),
        "cause_details": cause_details,
    });
    failure
}

fn map_atomic_exchange_error(err: std::io::Error) -> CommandFailure {
    if err.kind() == std::io::ErrorKind::Unsupported {
        CommandFailure::new(
            ErrorCode::ProjectionMethodUnsupported,
            format!(
                "atomic projection exchange is unavailable on this platform or filesystem: {err}"
            ),
        )
    } else {
        map_io(err)
    }
}

fn map_atomic_operation_error(err: std::io::Error, path: &Path) -> CommandFailure {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "projection path '{}' appeared during atomic activation; concurrent entry was preserved",
                path.display()
            ),
        ),
        std::io::ErrorKind::Unsupported => CommandFailure::new(
            ErrorCode::ProjectionMethodUnsupported,
            format!(
                "atomic projection activation is unavailable on this platform or filesystem: {err}"
            ),
        ),
        _ => map_io(err),
    }
}
