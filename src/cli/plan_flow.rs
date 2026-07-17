use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand};
use serde::Serialize;

use super::{AgentKind, ProjectionMethod, UseScope};

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum PlanCommand {
    #[command(about = "Create a durable plan for converging one skill change")]
    Converge(PlanConvergeArgs),

    #[command(about = "Create a durable plan for the skill use flow")]
    Use(PlanUseArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
#[command(group(
    ArgGroup::new("direction")
        .args(["from_source", "from_projection"])
        .multiple(false)
))]
pub struct PlanConvergeArgs {
    /// Registry skill id to converge.
    pub skill: String,

    /// Use the canonical registry source as the change input (the default).
    #[arg(long)]
    pub from_source: bool,

    /// Capture the change from one explicitly selected projection instance.
    #[arg(long, requires = "instance")]
    pub from_projection: bool,

    /// Projection instance used as input with --from-projection.
    #[arg(long, requires = "from_projection")]
    pub instance: Option<String>,

    /// Restrict active bindings to one agent.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Restrict active bindings to a matching workspace.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Restrict active bindings to one profile.
    #[arg(long)]
    pub profile: Option<String>,

    /// Require at least one selected active runtime projection.
    #[arg(long)]
    pub require_runtime: bool,

    /// Treat a post-apply restart requirement as accepted evidence.
    #[arg(long, requires = "require_runtime")]
    pub accept_restart_required: bool,

    /// Request registry remote push after the future local transaction.
    #[arg(long)]
    pub push_remote: bool,
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

    /// Reviewed digest required by convergence plans.
    #[arg(long)]
    pub plan_digest: Option<String>,

    /// Caller-provided idempotency key for safe retries.
    #[arg(long)]
    pub idempotency_key: String,

    /// Approval tokens required by the plan. Can be repeated or comma-separated.
    #[arg(long = "approve", value_delimiter = ',')]
    pub approvals: Vec<String>,
}
