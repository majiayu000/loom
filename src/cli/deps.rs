use std::path::PathBuf;

use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillDepsArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent config to inspect for MCP readiness.
    #[arg(long)]
    pub agent: Option<String>,

    /// Workspace path used for future binding-aware readiness.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}
