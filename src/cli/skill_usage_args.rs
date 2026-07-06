use std::path::PathBuf;

use clap::{Args, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillUsedArgs {
    pub skill: String,

    #[arg(long)]
    pub agent: Option<String>,

    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[arg(long)]
    pub session_id: Option<String>,

    #[arg(long)]
    pub tokens_in: Option<u64>,

    #[arg(long)]
    pub tokens_out: Option<u64>,

    #[arg(long)]
    pub commands: Option<u64>,

    #[arg(long)]
    pub duration_ms: Option<u64>,

    #[arg(long, conflicts_with = "error")]
    pub success: bool,

    #[arg(long, conflicts_with = "success")]
    pub error: bool,

    #[arg(long)]
    pub failure_category: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillFeedbackArgs {
    pub skill: String,

    #[arg(long, value_enum)]
    pub feedback: SkillFeedbackValue,

    #[arg(long)]
    pub agent: Option<String>,

    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[arg(long)]
    pub session_id: Option<String>,

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
