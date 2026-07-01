use std::path::PathBuf;

use clap::Args;
use serde::Serialize;

use super::AgentKind;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillDiagnoseArgs {
    /// Registry skill name.
    pub skill: String,

    /// Include agent-specific visibility checks. Currently supports codex.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,
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
