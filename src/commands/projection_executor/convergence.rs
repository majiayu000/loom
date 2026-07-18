use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fs_util::{exchange_paths_atomic, remove_path_if_exists, rename_no_replace_atomic};
use crate::state_model::RegistryProjectionInstance;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::super::projections::{apply_projection_observation, observe_projection_from_source};
use super::staging_cleanup::{CleanupClaim, claim_for_cleanup, path_entry_exists};

mod activation;
mod ownership;
mod validation;
#[cfg(test)]
pub(crate) use activation::activate_after_mutation;
pub(crate) use activation::{activate_prepared_projection, discard_prepared_projection};
pub(super) use ownership::{map_ownership_fingerprint_error, projection_ownership_fingerprint};
pub(crate) use validation::{
    validate_prepared_projection_artifact, validate_projection_artifact_layout,
    validate_projection_rollback_artifact_for_finalize,
    validate_projection_rollback_artifact_for_rollback,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PreparedProjectionArtifact {
    pub(crate) projection: RegistryProjectionInstance,
    pub(crate) source_path: PathBuf,
    pub(crate) staging_path: PathBuf,
    pub(crate) materialized_path: PathBuf,
    pub(crate) path_exists: bool,
    pub(crate) staging_digest: String,
    pub(crate) existing_digest: Option<String>,
}

#[must_use = "a prepared projection must be activated or explicitly discarded"]
pub(crate) struct PreparedProjection {
    parts: Option<PreparedProjectionArtifact>,
}

#[allow(
    dead_code,
    reason = "durable prepared evidence is consumed by the SP524-T004 transaction"
)]
impl PreparedProjection {
    pub(super) fn new(
        projection: RegistryProjectionInstance,
        source_path: PathBuf,
        staging_path: PathBuf,
        materialized_path: PathBuf,
        path_exists: bool,
        staging_digest: String,
        existing_digest: Option<String>,
    ) -> Self {
        Self {
            parts: Some(PreparedProjectionArtifact {
                projection,
                source_path,
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

    pub(crate) fn durable_artifact(&self) -> &PreparedProjectionArtifact {
        self.parts
            .as_ref()
            .expect("prepared projection must own its transaction state")
    }

    pub(crate) fn into_durable_artifact(mut self) -> PreparedProjectionArtifact {
        self.take_parts()
    }

    pub(crate) fn from_durable_artifact(artifact: PreparedProjectionArtifact) -> Self {
        Self {
            parts: Some(artifact),
        }
    }

    fn take_parts(&mut self) -> PreparedProjectionArtifact {
        self.parts
            .take()
            .expect("prepared projection must own its transaction state")
    }
}

impl Drop for PreparedProjection {
    fn drop(&mut self) {
        if let Some(parts) = self.parts.take()
            && let Err(err) = cleanup_prepared_artifact(&parts)
        {
            eprintln!(
                "loom: abandoned prepared projection requires recovery: {}; details={}",
                err.message, err.details
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PendingCleanupReason {
    RollbackExchanged,
    RollbackCreated,
    FinalizeExchanged,
}

impl PendingCleanupReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::RollbackExchanged => "rollback_exchanged",
            Self::RollbackCreated => "rollback_created",
            Self::FinalizeExchanged => "finalize_exchanged",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
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
        expected_live_digest: Option<String>,
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
                expected_live_digest,
                expected_digest,
                reason,
            } => json!({
                "reason": reason.as_str(),
                "kind": "pending_cleanup",
                "original_path": materialized_path.display().to_string(),
                "artifact_path": artifact_path.display().to_string(),
                "expected_live_digest": expected_live_digest,
                "expected_digest": expected_digest,
            }),
        }
    }

    pub(crate) fn rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        self.prepare_rollback()?;
        self.cleanup_pending()
    }

    pub(crate) fn prepare_rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        self.prepare_rollback_with_after_mutation(|| Ok(()))
    }

    fn prepare_rollback_with_after_mutation(
        &mut self,
        after_mutation: impl Fn() -> std::io::Result<()>,
    ) -> std::result::Result<(), CommandFailure> {
        match self.clone() {
            Self::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => {
                let live_digest = owned_digest(&materialized_path, self)?;
                let backup_digest = owned_digest(&backup_path, self)?;
                if live_digest.as_deref() == Some(&activated_digest)
                    && backup_digest.as_deref() == Some(&original_digest)
                {
                    exchange_paths_atomic(&backup_path, &materialized_path)
                        .map_err(map_atomic_exchange_error)
                        .map_err(|err| with_recovery_details(err, self))?;
                    after_mutation()
                        .map_err(map_io)
                        .map_err(|err| with_recovery_details(err, self))?;
                } else if live_digest.as_deref() != Some(&original_digest)
                    || backup_digest.as_deref() != Some(&activated_digest)
                {
                    return Err(rollback_state_mismatch(self, live_digest, backup_digest));
                }
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: backup_path,
                    expected_live_digest: Some(original_digest),
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
                let live_digest = owned_digest(&materialized_path, self)?;
                let rollback_digest = owned_digest(&rollback_path, self)?;
                if live_digest.as_deref() == Some(&activated_digest) && rollback_digest.is_none() {
                    rename_no_replace_atomic(&materialized_path, &rollback_path)
                        .map_err(|err| map_atomic_operation_error(err, &rollback_path))
                        .map_err(|err| with_recovery_details(err, self))?;
                    after_mutation()
                        .map_err(map_io)
                        .map_err(|err| with_recovery_details(err, self))?;
                } else if live_digest.is_some()
                    || rollback_digest.as_deref() != Some(&activated_digest)
                {
                    return Err(rollback_state_mismatch(self, live_digest, rollback_digest));
                }
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: rollback_path,
                    expected_live_digest: None,
                    expected_digest: activated_digest,
                    reason: PendingCleanupReason::RollbackCreated,
                };
                Ok(())
            }
            Self::PendingCleanup {
                reason:
                    PendingCleanupReason::RollbackExchanged | PendingCleanupReason::RollbackCreated,
                ..
            } => Ok(()),
            Self::PendingCleanup { .. } => Err(invalid_recovery_transition("rollback", self)),
        }
    }

    #[cfg(test)]
    pub(crate) fn rollback_after_mutation(
        &mut self,
        after_mutation: impl Fn() -> std::io::Result<()>,
    ) -> std::result::Result<(), CommandFailure> {
        self.prepare_rollback_with_after_mutation(after_mutation)
    }

    pub(crate) fn finalize(&mut self) -> std::result::Result<(), CommandFailure> {
        self.prepare_finalize()?;
        if matches!(self, Self::PendingCleanup { .. }) {
            self.cleanup_pending()
        } else {
            Ok(())
        }
    }

    pub(crate) fn prepare_finalize(&mut self) -> std::result::Result<(), CommandFailure> {
        self.prepare_finalize_with_after_claim(|_| Ok(()))
    }

    fn prepare_finalize_with_after_claim(
        &mut self,
        after_claim: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> std::result::Result<(), CommandFailure> {
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
                let original_artifact = self.clone();
                let backup_name = backup_path
                    .file_name()
                    .ok_or_else(|| {
                        with_recovery_details(
                            CommandFailure::new(
                                ErrorCode::StateCorrupt,
                                "projection backup path has no file name",
                            ),
                            self,
                        )
                    })?
                    .to_string_lossy();
                let claim_path =
                    backup_path.with_file_name(format!("{backup_name}.finalize-claim"));
                let claim_exists = path_entry_exists(&claim_path)
                    .map_err(map_io)
                    .map_err(|err| with_recovery_details(err, self))?;
                let backup_exists = path_entry_exists(&backup_path)
                    .map_err(map_io)
                    .map_err(|err| with_recovery_details(err, self))?;
                if claim_exists && !backup_exists {
                    *self = Self::PendingCleanup {
                        materialized_path,
                        artifact_path: claim_path,
                        expected_live_digest: Some(activated_digest),
                        expected_digest: original_digest,
                        reason: PendingCleanupReason::FinalizeExchanged,
                    };
                    return Ok(());
                }
                rename_no_replace_atomic(&backup_path, &claim_path)
                    .map_err(|err| map_atomic_operation_error(err, &claim_path))
                    .map_err(|err| with_recovery_details(err, self))?;
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: claim_path.clone(),
                    expected_live_digest: Some(activated_digest),
                    expected_digest: original_digest.clone(),
                    reason: PendingCleanupReason::FinalizeExchanged,
                };
                after_claim(&backup_path)
                    .map_err(map_io)
                    .map_err(|err| with_recovery_details(err, self))?;
                if let Err(mut validation) = validate_owned_digest(
                    &claim_path,
                    &original_digest,
                    "finalize claimed projection backup",
                    self,
                ) {
                    match rename_no_replace_atomic(&claim_path, &backup_path) {
                        Ok(()) => {
                            *self = original_artifact;
                            validation.details["claim_restore"] = json!({
                                "status": "restored",
                                "path": backup_path.display().to_string(),
                            });
                            validation.details["artifact"] = self.evidence();
                        }
                        Err(restore_error) => {
                            validation.details["claim_restore"] = json!({
                                "status": "preserved_at_claim",
                                "claim_path": claim_path.display().to_string(),
                                "original_path": backup_path.display().to_string(),
                                "error": restore_error.to_string(),
                            });
                        }
                    }
                    return Err(validation);
                }
                Ok(())
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
            Self::PendingCleanup {
                reason: PendingCleanupReason::FinalizeExchanged,
                ..
            } => Ok(()),
            Self::PendingCleanup { .. } => Err(invalid_recovery_transition("finalize", self)),
        }
    }

    #[cfg(test)]
    pub(crate) fn finalize_after_claim(
        &mut self,
        after_claim: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> std::result::Result<(), CommandFailure> {
        self.prepare_finalize_with_after_claim(after_claim)
    }

    pub(crate) fn cleanup_pending(&mut self) -> std::result::Result<(), CommandFailure> {
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
        let claim_path = match claim_for_cleanup(&artifact_path, ".pending-cleanup-claim")
            .map_err(map_io)
            .map_err(|err| with_recovery_details(err, self))?
        {
            CleanupClaim::Missing => return Ok(()),
            CleanupClaim::Claimed(path) => path,
        };
        validate_owned_digest(
            &claim_path,
            &expected_digest,
            "clean projection rollback artifact",
            self,
        )?;
        remove_path_if_exists(&claim_path)
            .map_err(map_io)
            .map_err(|err| with_recovery_details(err, self))
    }
}

fn owned_digest(
    path: &Path,
    artifact: &ProjectionRollbackArtifact,
) -> std::result::Result<Option<String>, CommandFailure> {
    if !path_entry_exists(path)
        .map_err(map_io)
        .map_err(|err| with_recovery_details(err, artifact))?
    {
        return Ok(None);
    }
    projection_ownership_fingerprint(path)
        .map(Some)
        .map_err(|err| with_recovery_details(map_ownership_fingerprint_error(err, path), artifact))
}

fn rollback_state_mismatch(
    artifact: &ProjectionRollbackArtifact,
    live_digest: Option<String>,
    rollback_digest: Option<String>,
) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        "cannot identify a safe projection rollback state; concurrent data was preserved",
    );
    failure.details = json!({
        "live_digest": live_digest,
        "rollback_digest": rollback_digest,
    });
    with_recovery_details(failure, artifact)
}

fn invalid_recovery_transition(
    operation: &str,
    artifact: &ProjectionRollbackArtifact,
) -> CommandFailure {
    with_recovery_details(
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("cannot {operation} projection artifact after opposite transition"),
        ),
        artifact,
    )
}

#[must_use = "an activated projection must be finalized or rolled back"]
pub(crate) struct ProjectionActivationOutput {
    projection: Option<RegistryProjectionInstance>,
    rollback_artifact: Option<ProjectionRollbackArtifact>,
    #[cfg(test)]
    fail_cleanup_once: bool,
}

#[allow(
    dead_code,
    reason = "durable rollback evidence is consumed by the SP524-T004 transaction"
)]
impl ProjectionActivationOutput {
    pub(crate) fn projection(&self) -> &RegistryProjectionInstance {
        self.projection
            .as_ref()
            .expect("activated projection must own its projection identity")
    }

    pub(crate) fn rollback_evidence(&self) -> Value {
        self.rollback_artifact
            .as_ref()
            .expect("activated projection must own a rollback artifact")
            .evidence()
    }

    pub(crate) fn durable_rollback_artifact(&self) -> &ProjectionRollbackArtifact {
        self.rollback_artifact
            .as_ref()
            .expect("activated projection must own a rollback artifact")
    }

    pub(crate) fn into_durable_parts(
        mut self,
    ) -> (RegistryProjectionInstance, ProjectionRollbackArtifact) {
        let projection = self
            .projection
            .take()
            .expect("activated projection must own its projection identity");
        let artifact = self
            .rollback_artifact
            .take()
            .expect("activated projection must own a rollback artifact");
        (projection, artifact)
    }

    pub(crate) fn from_durable_parts(
        projection: RegistryProjectionInstance,
        artifact: ProjectionRollbackArtifact,
    ) -> Self {
        Self {
            projection: Some(projection),
            rollback_artifact: Some(artifact),
            #[cfg(test)]
            fail_cleanup_once: false,
        }
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
        artifact.prepare_rollback()?;
        self.projection = None;
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
        if matches!(
            artifact,
            ProjectionRollbackArtifact::PendingCleanup {
                reason: PendingCleanupReason::RollbackExchanged
                    | PendingCleanupReason::RollbackCreated,
                ..
            }
        ) {
            return Err(with_recovery_details(
                CommandFailure::new(
                    ErrorCode::ProjectionConflict,
                    "cannot finalize after rollback took effect; retry rollback cleanup",
                ),
                artifact,
            ));
        }
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

fn cleanup_prepare_failure(
    err: CommandFailure,
    parts: &PreparedProjectionArtifact,
) -> CommandFailure {
    let mut cleanup_errors = Vec::new();
    if let Err(cleanup_err) = cleanup_prepared_artifact(parts) {
        cleanup_errors.push(json!({
            "step": "remove_projection_staging",
            "code": cleanup_err.code.as_str(),
            "message": cleanup_err.message,
            "details": cleanup_err.details,
        }));
    }
    err.with_rollback_errors(cleanup_errors)
}

fn cleanup_prepared_artifact(
    parts: &PreparedProjectionArtifact,
) -> std::result::Result<(), CommandFailure> {
    let claim_path = match claim_for_cleanup(&parts.staging_path, ".prepared-cleanup-claim")
        .map_err(map_io)
        .map_err(|err| with_prepared_recovery_details(err, parts))?
    {
        CleanupClaim::Missing => return Ok(()),
        CleanupClaim::Claimed(path) => path,
    };
    validate_prepared_digest(
        &claim_path,
        &parts.staging_digest,
        "prepared staging projection",
    )
    .map_err(|err| with_prepared_recovery_details(err, parts))?;
    remove_path_if_exists(&claim_path)
        .map_err(map_io)
        .map_err(|err| with_prepared_recovery_details(err, parts))
}

fn with_prepared_recovery_details(
    mut failure: CommandFailure,
    parts: &PreparedProjectionArtifact,
) -> CommandFailure {
    let cause_details = std::mem::replace(&mut failure.details, json!({}));
    failure.details = json!({
        "recovery_required": true,
        "artifact": {
            "projection_instance_id": parts.projection.instance_id,
            "skill_id": parts.projection.skill_id,
            "binding_id": parts.projection.binding_id,
            "target_id": parts.projection.target_id,
            "source_path": parts.source_path.display().to_string(),
            "staging_path": parts.staging_path.display().to_string(),
            "materialized_path": parts.materialized_path.display().to_string(),
            "path_exists": parts.path_exists,
            "staging_digest": parts.staging_digest,
            "existing_digest": parts.existing_digest,
        },
        "cause_details": cause_details,
    });
    failure
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
