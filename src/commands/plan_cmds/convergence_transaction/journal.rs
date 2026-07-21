use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ownership_state::OwnershipAttempt;
use super::registry_commit::RegistryIndexAttempt;
use crate::state_model::{RegistryOperationRecord, RegistryOpsCheckpoint, RegistryProjectionsFile};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct TransactionJournal {
    pub(super) plan_id: String,
    pub(super) plan_digest: String,
    pub(super) convergence_id: String,
    pub(super) idempotency_key_digest: String,
    pub(super) idempotency_binding_digest: String,
    pub(super) skill: String,
    pub(super) previous_head: String,
    pub(super) artifact_root: String,
    pub(super) artifact_owner_proof: String,
    pub(super) ownership_attempts: Vec<OwnershipAttempt>,
    pub(super) index_backup: String,
    pub(super) index_backup_digest: Option<String>,
    pub(super) source_backup: Option<Value>,
    pub(super) source_staging: Option<String>,
    pub(super) source_owner_proof: Option<String>,
    #[serde(default)]
    pub(super) source_activated_fingerprint: Option<String>,
    pub(super) projections: Vec<ProjectionBackup>,
    pub(super) original_projections: RegistryProjectionsFile,
    #[serde(default)]
    pub(super) original_operations: Option<Vec<RegistryOperationRecord>>,
    #[serde(default)]
    pub(super) original_checkpoint: Option<RegistryOpsCheckpoint>,
    pub(super) installed_projections: usize,
    pub(super) expected_projections: Option<RegistryProjectionsFile>,
    pub(super) source_head: Option<String>,
    pub(super) source_commit: Option<String>,
    pub(super) source_staged_index_digest: Option<String>,
    #[serde(default)]
    pub(super) source_index_changed: Option<bool>,
    #[serde(default)]
    pub(super) registry_commit: Option<String>,
    #[serde(default)]
    pub(super) registry_staged_index_digest: Option<String>,
    #[serde(default)]
    pub(super) registry_index_attempts: Vec<RegistryIndexAttempt>,
    pub(super) rollback_head: Option<String>,
    pub(super) rollback_index_digest: Option<String>,
    #[serde(default)]
    pub(super) preparation_aborted: bool,
    pub(super) aggregate_operation_id: Option<String>,
    pub(super) aggregate_evidence: Option<Value>,
    #[serde(default)]
    pub(super) aggregate_operation: Option<RegistryOperationRecord>,
    #[serde(default)]
    pub(super) aggregate_checkpoint: Option<RegistryOpsCheckpoint>,
    pub(super) result: Option<Value>,
    pub(super) phase: TransactionPhase,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum TransactionPhase {
    Preparing,
    Prepared,
    ReplacingSource,
    SourceReplaced,
    CommittingSource,
    SourceCommitted,
    RotatingProjections,
    PreparingProjections,
    InstallingProjections,
    ProjectionsSwapped,
    CommittingRegistry,
    RollingBack,
    CommittedCleanupPending,
    RolledBackCleanupPending,
    CommittedArtifactsRetained,
    RolledBackArtifactsRetained,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ProjectionBackup {
    pub(super) materialized_path: String,
    pub(super) backup: Option<Value>,
    pub(super) staging_owner: String,
    pub(super) owner_proof: String,
    pub(super) staging_path: String,
    #[serde(default)]
    pub(super) activated_fingerprint: Option<String>,
    #[serde(default)]
    pub(super) activated: bool,
    #[serde(default)]
    pub(super) activation_pending: bool,
    #[serde(default)]
    pub(super) original_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) restored_fingerprint: Option<String>,
    #[serde(default)]
    pub(super) restore_pending: bool,
}
