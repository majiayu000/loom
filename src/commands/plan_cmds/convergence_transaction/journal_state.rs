use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state_model::RegistryProjectionInstance;

use super::{PreparedProjectionArtifact, ProjectionRollbackArtifact};
use super::{TransactionJournal, TransactionPhase};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProjectionTransactionState {
    #[default]
    Declared,
    Prepared,
    NoopPrepared,
    Activated,
    RollbackCleanupPending,
    RolledBack,
    FinalizeCleanupPending,
    Finalized,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DeclaredPathKind {
    Directory,
    File,
    Symlink,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DeclaredPathBackupEvidence {
    pub(super) kind: DeclaredPathKind,
    pub(super) original_path: PathBuf,
    pub(super) backup_path: PathBuf,
}

impl DeclaredPathBackupEvidence {
    pub(super) fn from_existing_path(
        original_path: &Path,
        backup_path: &Path,
    ) -> std::io::Result<Option<Self>> {
        let metadata = match std::fs::symlink_metadata(original_path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        let kind = if metadata.file_type().is_symlink() {
            DeclaredPathKind::Symlink
        } else if metadata.is_dir() {
            DeclaredPathKind::Directory
        } else {
            DeclaredPathKind::File
        };
        Ok(Some(Self {
            kind,
            original_path: original_path.to_path_buf(),
            backup_path: backup_path.to_path_buf(),
        }))
    }

    pub(super) fn as_legacy_value(&self) -> Value {
        let kind = match self.kind {
            DeclaredPathKind::Directory => "dir",
            DeclaredPathKind::File => "file",
            DeclaredPathKind::Symlink => "symlink",
        };
        json!({
            "kind": kind,
            "original_path": self.original_path,
            "backup_path": self.backup_path,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct ProjectionBackup {
    pub(super) materialized_path: String,
    pub(super) staging_owner: String,
    pub(super) owner_proof: String,
    pub(super) staging_path: String,
    #[serde(default)]
    pub(super) prepared: Option<PreparedProjectionArtifact>,
    #[serde(default)]
    pub(super) rollback: Option<ProjectionRollbackArtifact>,
    #[serde(default)]
    pub(super) projection: Option<RegistryProjectionInstance>,
    #[serde(default)]
    pub(super) state: ProjectionTransactionState,
}

pub(super) fn validate_projection_states(journal: &TransactionJournal) -> bool {
    let installed = journal.installed_projections;
    journal
        .projections
        .iter()
        .enumerate()
        .all(|(index, projection)| {
            let shape = match projection.state {
                ProjectionTransactionState::Declared => {
                    projection.prepared.is_none()
                        && projection.rollback.is_none()
                        && projection.projection.is_none()
                }
                ProjectionTransactionState::Prepared => {
                    projection.prepared.is_some()
                        && projection.rollback.is_none()
                        && projection.projection.is_some()
                }
                ProjectionTransactionState::NoopPrepared => {
                    projection.prepared.is_none()
                        && projection.rollback.is_none()
                        && projection.projection.is_some()
                }
                ProjectionTransactionState::Activated => {
                    projection.prepared.is_none() && projection.projection.is_some()
                }
                ProjectionTransactionState::RollbackCleanupPending
                | ProjectionTransactionState::FinalizeCleanupPending => {
                    projection.prepared.is_none()
                        && projection.rollback.is_some()
                        && projection.projection.is_some()
                }
                ProjectionTransactionState::RolledBack | ProjectionTransactionState::Finalized => {
                    projection.rollback.is_none()
                }
            };
            shape
                && match journal.phase {
                    TransactionPhase::Preparing => matches!(
                        projection.state,
                        ProjectionTransactionState::Declared
                            | ProjectionTransactionState::Prepared
                            | ProjectionTransactionState::NoopPrepared
                    ),
                    TransactionPhase::Prepared
                    | TransactionPhase::ReplacingSource
                    | TransactionPhase::SourceReplaced
                    | TransactionPhase::CommittingSource
                    | TransactionPhase::SourceCommitted => matches!(
                        projection.state,
                        ProjectionTransactionState::Prepared
                            | ProjectionTransactionState::NoopPrepared
                    ),
                    TransactionPhase::InstallingProjections => {
                        if index < installed {
                            projection.state == ProjectionTransactionState::Activated
                        } else {
                            matches!(
                                projection.state,
                                ProjectionTransactionState::Prepared
                                    | ProjectionTransactionState::NoopPrepared
                            )
                        }
                    }
                    TransactionPhase::ProjectionsSwapped | TransactionPhase::CommittingRegistry => {
                        projection.state == ProjectionTransactionState::Activated
                    }
                    TransactionPhase::RollingBack => matches!(
                        projection.state,
                        ProjectionTransactionState::Activated
                            | ProjectionTransactionState::RollbackCleanupPending
                            | ProjectionTransactionState::RolledBack
                    ),
                    TransactionPhase::CommittedCleanupPending => matches!(
                        projection.state,
                        ProjectionTransactionState::Activated
                            | ProjectionTransactionState::FinalizeCleanupPending
                            | ProjectionTransactionState::Finalized
                    ),
                    TransactionPhase::RolledBackCleanupPending => {
                        projection.state == ProjectionTransactionState::RolledBack
                    }
                }
        })
}
