use std::path::PathBuf;

use clap::{Args, ValueEnum};
use serde::Serialize;

use super::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SkillDiagnoseCheck {
    All,
    Drift,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillDiagnoseArgs {
    /// Registry skill name.
    pub skill: String,

    /// Include agent-specific visibility checks. Currently supports codex.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Restrict diagnosis to one check family.
    #[arg(long, value_enum, default_value_t = SkillDiagnoseCheck::All)]
    pub check: SkillDiagnoseCheck,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillVisibilityArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent active view to inspect. Currently supports codex.
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    /// Optional project workspace used to match project-scoped Codex targets.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Optional profile id used to narrow active bindings.
    #[arg(long)]
    pub profile: Option<String>,
}
