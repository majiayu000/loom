use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state_model::RegistryProjectionsFile;

use super::ownership_state::OwnershipAttempt;
use super::registry_commit;

#[derive(Debug, Serialize, Deserialize)]
pub(in crate::commands::plan_cmds::convergence_transaction) struct TransactionJournal {
    pub(in crate::commands::plan_cmds::convergence_transaction) plan_id: String,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) plan_digest: Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) convergence_id: Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) idempotency_key_digest:
        Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) idempotency_binding_digest:
        Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) skill: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) previous_head: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) artifact_root: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) artifact_owner_proof: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) ownership_attempts:
        Vec<OwnershipAttempt>,
    pub(in crate::commands::plan_cmds::convergence_transaction) index_backup: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) index_backup_digest: Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) source_backup: Option<Value>,
    pub(in crate::commands::plan_cmds::convergence_transaction) source_staging: Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) source_owner_proof: Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) source_activated_fingerprint:
        Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) projections: Vec<ProjectionBackup>,
    pub(in crate::commands::plan_cmds::convergence_transaction) original_projections:
        RegistryProjectionsFile,
    pub(in crate::commands::plan_cmds::convergence_transaction) installed_projections: usize,
    pub(in crate::commands::plan_cmds::convergence_transaction) expected_projections:
        Option<RegistryProjectionsFile>,
    pub(in crate::commands::plan_cmds::convergence_transaction) source_head: Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) source_commit: Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) source_staged_index_digest:
        Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) source_index_changed: Option<bool>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) registry_commit: Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) registry_staged_index_digest:
        Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) registry_index_attempts:
        Vec<registry_commit::RegistryIndexAttempt>,
    pub(in crate::commands::plan_cmds::convergence_transaction) rollback_head: Option<String>,
    pub(in crate::commands::plan_cmds::convergence_transaction) rollback_index_digest:
        Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) preparation_aborted: bool,
    pub(in crate::commands::plan_cmds::convergence_transaction) result: Option<Value>,
    pub(in crate::commands::plan_cmds::convergence_transaction) phase: TransactionPhase,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(in crate::commands::plan_cmds::convergence_transaction) enum TransactionPhase {
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
pub(in crate::commands::plan_cmds::convergence_transaction) struct ProjectionBackup {
    pub(in crate::commands::plan_cmds::convergence_transaction) materialized_path: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) backup: Option<Value>,
    pub(in crate::commands::plan_cmds::convergence_transaction) staging_owner: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) owner_proof: String,
    pub(in crate::commands::plan_cmds::convergence_transaction) staging_path: String,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) activated_fingerprint:
        Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) activated: bool,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) activation_pending: bool,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) original_fingerprint:
        Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::commands::plan_cmds::convergence_transaction) restored_fingerprint:
        Option<String>,
    #[serde(default)]
    pub(in crate::commands::plan_cmds::convergence_transaction) restore_pending: bool,
}
