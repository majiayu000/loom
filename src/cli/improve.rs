use std::path::PathBuf;

use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillImproveArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent-specific checks to include when available.
    #[arg(long)]
    pub agent: Option<String>,

    /// Workspace path used for dependency readiness context.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Baseline ref used for drift and regression comparison.
    #[arg(long, default_value = "HEAD")]
    pub baseline: String,

    /// Include real-agent eval planning. Network/agent execution is never run by default.
    #[arg(long)]
    pub real_eval: bool,

    /// Explicitly request the default read-only behavior.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillRegressionArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent-specific checks to include when available.
    #[arg(long)]
    pub agent: Option<String>,

    /// Older baseline ref.
    #[arg(long = "from", default_value = "HEAD")]
    pub from_ref: String,

    /// Newer ref to compare, or working-tree for local edits.
    #[arg(long = "to", default_value = "working-tree")]
    pub to_ref: String,
}
