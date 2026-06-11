use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ArgInvalid,
    DependencyConflict,
    SchemaMismatch,
    StateCorrupt,
    StateNotInitialized,
    SkillNotFound,
    BindingNotFound,
    TargetNotFound,
    TrashEntryNotFound,
    TargetNotManaged,
    TargetAgentMismatch,
    ProjectionConflict,
    ProjectionMethodUnsupported,
    CaptureConflict,
    AuditError,
    LockBusy,
    RemoteUnreachable,
    RemoteDiverged,
    PushRejected,
    ReplayConflict,
    QueueBlocked,
    GitError,
    IoError,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ArgInvalid => "ARG_INVALID",
            Self::DependencyConflict => "DEPENDENCY_CONFLICT",
            Self::SchemaMismatch => "SCHEMA_MISMATCH",
            Self::StateCorrupt => "STATE_CORRUPT",
            Self::StateNotInitialized => "STATE_NOT_INITIALIZED",
            Self::SkillNotFound => "SKILL_NOT_FOUND",
            Self::BindingNotFound => "BINDING_NOT_FOUND",
            Self::TargetNotFound => "TARGET_NOT_FOUND",
            Self::TrashEntryNotFound => "TRASH_ENTRY_NOT_FOUND",
            Self::TargetNotManaged => "TARGET_NOT_MANAGED",
            Self::TargetAgentMismatch => "TARGET_AGENT_MISMATCH",
            Self::ProjectionConflict => "PROJECTION_CONFLICT",
            Self::ProjectionMethodUnsupported => "PROJECTION_METHOD_UNSUPPORTED",
            Self::CaptureConflict => "CAPTURE_CONFLICT",
            Self::AuditError => "AUDIT_ERROR",
            Self::LockBusy => "LOCK_BUSY",
            Self::RemoteUnreachable => "REMOTE_UNREACHABLE",
            Self::RemoteDiverged => "REMOTE_DIVERGED",
            Self::PushRejected => "PUSH_REJECTED",
            Self::ReplayConflict => "REPLAY_CONFLICT",
            Self::QueueBlocked => "QUEUE_BLOCKED",
            Self::GitError => "GIT_ERROR",
            Self::IoError => "IO_ERROR",
            Self::InternalError => "INTERNAL_ERROR",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Self::ArgInvalid => 2,
            Self::DependencyConflict => 3,
            Self::SchemaMismatch => 3,
            Self::StateCorrupt => 3,
            Self::StateNotInitialized => 3,
            Self::LockBusy => 4,
            Self::RemoteUnreachable => 10,
            Self::RemoteDiverged => 10,
            Self::PushRejected => 10,
            Self::ReplayConflict => 10,
            Self::QueueBlocked => 10,
            Self::GitError => 5,
            Self::IoError => 5,
            Self::InternalError => 3,
            Self::SkillNotFound => 3,
            Self::BindingNotFound => 3,
            Self::TargetNotFound => 3,
            Self::TrashEntryNotFound => 3,
            Self::TargetNotManaged => 3,
            Self::TargetAgentMismatch => 3,
            Self::ProjectionConflict => 3,
            Self::ProjectionMethodUnsupported => 3,
            Self::CaptureConflict => 3,
            Self::AuditError => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncState {
    Synced,
    PendingPush,
    Diverged,
    Conflicted,
    LocalOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_id: Option<String>,
    pub request_id: String,
    pub command: String,
    pub created_at: DateTime<Utc>,
    pub details: serde_json::Value,
}

impl PendingOp {
    pub fn new(command: &str, details: serde_json::Value, request_id: String) -> Self {
        Self {
            op_id: Some(uuid::Uuid::new_v4().to_string()),
            request_id,
            command: command.to_string(),
            created_at: Utc::now(),
            details,
        }
    }

    pub fn stable_id(&self) -> String {
        self.op_id.clone().unwrap_or_else(|| {
            format!(
                "legacy:{}:{}:{}",
                self.request_id,
                self.command,
                self.created_at.timestamp_micros()
            )
        })
    }
}
