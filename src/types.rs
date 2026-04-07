use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ArgInvalid,
    UnsupportedV1Command,
    SkillNotFound,
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
            Self::UnsupportedV1Command => "UNSUPPORTED_V1_COMMAND",
            Self::SkillNotFound => "SKILL_NOT_FOUND",
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
            Self::UnsupportedV1Command => 2,
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
    pub request_id: String,
    pub command: String,
    pub created_at: DateTime<Utc>,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTargetConfig {
    pub method: String,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetsState {
    pub skills: std::collections::BTreeMap<String, SkillTargetConfig>,
}
