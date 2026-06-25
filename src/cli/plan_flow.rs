use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

use super::{AgentKind, ProjectionMethod, UseScope};

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum PlanCommand {
    #[command(about = "Create a durable plan for the skill use flow")]
    Use(PlanUseArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PlanUseArgs {
    /// Registry skill id to use.
    pub skill: String,

    /// Comma-separated agents to prepare, for example claude,codex.
    #[arg(long, value_enum, value_delimiter = ',')]
    pub agents: Vec<AgentKind>,

    /// Scope for target and binding creation.
    #[arg(long, value_enum, default_value_t = UseScope::Project)]
    pub scope: UseScope,

    /// Workspace path used for project scope matching. Defaults to current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Profile label for the generated or reused binding.
    #[arg(long, default_value = "default")]
    pub profile: String,

    /// Projection method to use when applying.
    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,

    /// Base directory for managed targets. Defaults to <root>/targets/<scope>.
    #[arg(long)]
    pub target_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ApplyArgs {
    /// Durable plan id returned by `loom plan`.
    pub plan_id: String,

    /// Caller-provided idempotency key for safe retries.
    #[arg(long)]
    pub idempotency_key: String,

    /// Approval tokens required by the plan. Can be repeated or comma-separated.
    #[arg(long = "approve", value_delimiter = ',')]
    pub approvals: Vec<String>,
}
