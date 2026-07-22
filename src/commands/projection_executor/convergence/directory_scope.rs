use std::io;
use std::path::{Path, PathBuf};

use crate::fs_util::DirectoryHandle;

use super::PreparedProjectionArtifact;

pub(crate) struct PreparedProjectionScope {
    target_directory: DirectoryHandle,
    owner_directory: DirectoryHandle,
    live_name: PathBuf,
    staging_name: PathBuf,
    owner_path: PathBuf,
}

impl PreparedProjectionScope {
    pub(crate) fn new(
        target_directory: DirectoryHandle,
        owner_directory: DirectoryHandle,
        live_name: PathBuf,
        staging_name: PathBuf,
        owner_path: PathBuf,
    ) -> Self {
        Self {
            target_directory,
            owner_directory,
            live_name,
            staging_name,
            owner_path,
        }
    }

    pub(super) fn open(parts: &PreparedProjectionArtifact) -> io::Result<Self> {
        let target_path = parts.materialized_path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "live path has no parent")
        })?;
        let owner_path = parts
            .staging_path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stage has no owner"))?;
        let target_directory = DirectoryHandle::open(target_path)?;
        let owner_directory = if owner_path == target_path {
            target_directory.try_clone()?
        } else {
            if owner_path.parent() != Some(target_path) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "stage owner is not inside the live target directory",
                ));
            }
            let owner_name = owner_path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "owner has no file name")
            })?;
            target_directory.open_dir(Path::new(owner_name))?
        };
        let live_name = parts
            .materialized_path
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "live path has no file name")
            })?;
        let staging_name = parts
            .staging_path
            .file_name()
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "stage has no file name"))?;
        Ok(Self::new(
            target_directory,
            owner_directory,
            live_name,
            staging_name,
            owner_path.to_path_buf(),
        ))
    }

    pub(super) fn exchange(&self) -> io::Result<()> {
        self.owner_directory.exchange_to(
            &self.staging_name,
            &self.target_directory,
            &self.live_name,
        )
    }

    pub(super) fn activate_create(&self) -> io::Result<()> {
        self.owner_directory.rename_no_replace_to(
            &self.staging_name,
            &self.target_directory,
            &self.live_name,
        )
    }

    pub(super) fn rollback_create(&self) -> io::Result<()> {
        self.target_directory.rename_no_replace_to(
            &self.live_name,
            &self.owner_directory,
            &self.staging_name,
        )
    }

    pub(super) fn prepare_rollback(
        &self,
        artifact: &mut super::ProjectionRollbackArtifact,
    ) -> Result<(), super::CommandFailure> {
        match artifact.clone() {
            super::ProjectionRollbackArtifact::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => {
                let live = super::owned_digest(&materialized_path, artifact)?;
                let backup = super::owned_digest(&backup_path, artifact)?;
                if live.as_deref() == Some(&activated_digest)
                    && backup.as_deref() == Some(&original_digest)
                {
                    self.exchange().map_err(super::map_atomic_exchange_error)?;
                } else if live.as_deref() != Some(&original_digest)
                    || backup.as_deref() != Some(&activated_digest)
                {
                    return Err(super::rollback_state_mismatch(artifact, live, backup));
                }
                *artifact = super::ProjectionRollbackArtifact::PendingCleanup {
                    materialized_path,
                    artifact_path: backup_path,
                    expected_digest: activated_digest,
                    reason: super::PendingCleanupReason::RollbackExchanged,
                };
                Ok(())
            }
            super::ProjectionRollbackArtifact::Created {
                materialized_path,
                rollback_path,
                activated_digest,
            } => {
                let live = super::owned_digest(&materialized_path, artifact)?;
                let rollback = super::owned_digest(&rollback_path, artifact)?;
                if live.as_deref() == Some(&activated_digest) && rollback.is_none() {
                    self.rollback_create().map_err(|error| {
                        super::map_atomic_operation_error(error, &rollback_path)
                    })?;
                } else if live.is_some() || rollback.as_deref() != Some(&activated_digest) {
                    return Err(super::rollback_state_mismatch(artifact, live, rollback));
                }
                *artifact = super::ProjectionRollbackArtifact::PendingCleanup {
                    materialized_path,
                    artifact_path: rollback_path,
                    expected_digest: activated_digest,
                    reason: super::PendingCleanupReason::RollbackCreated,
                };
                Ok(())
            }
            super::ProjectionRollbackArtifact::PendingCleanup {
                reason:
                    super::PendingCleanupReason::RollbackExchanged
                    | super::PendingCleanupReason::RollbackCreated,
                ..
            } => Ok(()),
            super::ProjectionRollbackArtifact::PendingCleanup { .. } => {
                Err(super::invalid_recovery_transition("rollback", artifact))
            }
        }
    }

    pub(super) fn finalize(
        &self,
        artifact: &mut super::ProjectionRollbackArtifact,
    ) -> Result<(), super::CommandFailure> {
        match artifact.clone() {
            super::ProjectionRollbackArtifact::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => {
                super::validate_owned_digest(
                    &materialized_path,
                    &activated_digest,
                    "finalize live projection",
                    artifact,
                )?;
                let backup_name = self.owner_entry(&backup_path)?;
                let claim_name =
                    PathBuf::from(format!("{}.finalize-claim", backup_name.to_string_lossy()));
                let stage_exists = self
                    .owner_directory
                    .entry_exists(&backup_name)
                    .map_err(super::map_io)?;
                let claim_exists = self
                    .owner_directory
                    .entry_exists(&claim_name)
                    .map_err(super::map_io)?;
                if stage_exists == claim_exists {
                    return Err(super::with_recovery_details(
                        super::CommandFailure::new(
                            crate::types::ErrorCode::ProjectionConflict,
                            "projection backup claim state is ambiguous",
                        ),
                        artifact,
                    ));
                }
                if stage_exists {
                    self.owner_directory
                        .rename_no_replace_to(&backup_name, &self.owner_directory, &claim_name)
                        .map_err(|error| super::map_atomic_operation_error(error, &backup_path))?;
                }
                let claim_path = self.owner_path.join(&claim_name);
                if let Err(validation) = super::validate_owned_digest(
                    &claim_path,
                    &original_digest,
                    "finalize claimed projection backup",
                    artifact,
                ) {
                    self.owner_directory
                        .rename_no_replace_to(&claim_name, &self.owner_directory, &backup_name)
                        .map_err(|error| super::map_atomic_operation_error(error, &backup_path))?;
                    return Err(validation);
                }
                *artifact = super::ProjectionRollbackArtifact::PendingCleanup {
                    materialized_path,
                    artifact_path: claim_path,
                    expected_digest: original_digest,
                    reason: super::PendingCleanupReason::FinalizeExchanged,
                };
                self.cleanup_pending(artifact)
            }
            super::ProjectionRollbackArtifact::Created {
                materialized_path,
                activated_digest,
                ..
            } => super::validate_owned_digest(
                &materialized_path,
                &activated_digest,
                "finalize live projection",
                artifact,
            ),
            super::ProjectionRollbackArtifact::PendingCleanup {
                reason: super::PendingCleanupReason::FinalizeExchanged,
                ..
            } => self.cleanup_pending(artifact),
            super::ProjectionRollbackArtifact::PendingCleanup { .. } => {
                Err(super::invalid_recovery_transition("finalize", artifact))
            }
        }
    }

    pub(super) fn cleanup_pending(
        &self,
        artifact: &mut super::ProjectionRollbackArtifact,
    ) -> Result<(), super::CommandFailure> {
        let (artifact_path, expected_digest) = match artifact {
            super::ProjectionRollbackArtifact::PendingCleanup {
                artifact_path,
                expected_digest,
                ..
            } => (artifact_path.clone(), expected_digest.clone()),
            _ => {
                return Err(super::CommandFailure::new(
                    crate::types::ErrorCode::InternalError,
                    "projection rollback artifact is not pending cleanup",
                ));
            }
        };
        let artifact_name = self.owner_entry(&artifact_path)?;
        let claim_name = PathBuf::from(format!(
            "{}.pending-cleanup-claim",
            artifact_name.to_string_lossy()
        ));
        let artifact_exists = self
            .owner_directory
            .entry_exists(&artifact_name)
            .map_err(super::map_io)?;
        let claim_exists = self
            .owner_directory
            .entry_exists(&claim_name)
            .map_err(super::map_io)?;
        if artifact_exists && claim_exists {
            return Err(super::with_recovery_details(
                super::CommandFailure::new(
                    crate::types::ErrorCode::ProjectionConflict,
                    "projection cleanup claim state is ambiguous",
                ),
                artifact,
            ));
        }
        if artifact_exists {
            self.owner_directory
                .rename_no_replace_to(&artifact_name, &self.owner_directory, &claim_name)
                .map_err(|error| super::map_atomic_operation_error(error, &artifact_path))?;
        } else if !claim_exists {
            return Ok(());
        }
        super::validate_owned_digest(
            &self.owner_path.join(claim_name),
            &expected_digest,
            "clean projection rollback artifact",
            artifact,
        )
    }

    fn owner_entry(&self, path: &Path) -> Result<PathBuf, super::CommandFailure> {
        if path.parent() != Some(self.owner_path.as_path()) {
            return Err(super::CommandFailure::new(
                crate::types::ErrorCode::StateCorrupt,
                "projection artifact escaped its opened owner directory",
            ));
        }
        path.file_name().map(PathBuf::from).ok_or_else(|| {
            super::CommandFailure::new(
                crate::types::ErrorCode::StateCorrupt,
                "projection artifact has no file name",
            )
        })
    }
}
