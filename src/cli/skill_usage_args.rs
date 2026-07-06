use std::path::PathBuf;

use clap::{Args, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillUsedArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent id that invoked the skill.
    #[arg(long)]
    pub agent: Option<String>,

    /// Workspace path used for hashed telemetry correlation.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// External session id. Stored only as a hash.
    #[arg(long)]
    pub session_id: Option<String>,

    /// Input token count for the invocation.
    #[arg(long)]
    pub tokens_in: Option<u64>,

    /// Output token count for the invocation.
    #[arg(long)]
    pub tokens_out: Option<u64>,

    /// Command count for the invocation.
    #[arg(long)]
    pub commands: Option<u64>,

    /// Invocation duration in milliseconds.
    #[arg(long)]
    pub duration_ms: Option<u64>,

    /// Record a successful skill invocation.
    #[arg(long, conflicts_with = "error")]
    pub success: bool,

    /// Record a failed skill invocation.
    #[arg(long, conflicts_with = "success")]
    pub error: bool,

    /// Structured failure category for --error.
    #[arg(long)]
    pub failure_category: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillFeedbackArgs {
    /// Registry skill name.
    pub skill: String,

    /// Explicit recommendation feedback.
    #[arg(long, value_enum)]
    pub feedback: SkillFeedbackValue,

    /// Agent id that received the recommendation.
    #[arg(long)]
    pub agent: Option<String>,

    /// Workspace path used for hashed telemetry correlation.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// External session id. Stored only as a hash.
    #[arg(long)]
    pub session_id: Option<String>,

    /// Task text for wrapper correlation. This value is not persisted in telemetry events.
    #[arg(long)]
    pub task: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillFeedbackValue {
    Accepted,
    Rejected,
    Ignored,
}

impl SkillFeedbackValue {
    pub fn telemetry_value(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Ignored => "ignored",
        }
    }
}
