use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand};
use serde::Serialize;

use super::{AgentKind, ProjectionMethod, UseScope};

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum PlanCommand {
    #[command(about = "Create a durable skill-convergence plan")]
    Converge(PlanConvergeArgs),

    #[command(about = "Create a durable skill-use plan")]
    Use(PlanUseArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
#[command(group(
    ArgGroup::new("direction")
        .args(["from_source", "from_projection"])
        .multiple(false)
))]
pub struct PlanConvergeArgs {
    /// Skill id.
    pub skill: String,

    /// Use registry source input (default).
    #[arg(long)]
    pub from_source: bool,

    /// Use one projection instance as input.
    #[arg(long, requires = "instance")]
    pub from_projection: bool,

    /// Input projection instance id.
    #[arg(long, requires = "from_projection")]
    pub instance: Option<String>,

    /// Filter bindings by agent.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Filter bindings by workspace.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Filter bindings by profile.
    #[arg(long)]
    pub profile: Option<String>,

    /// Require a selected runtime projection.
    #[arg(long)]
    pub require_runtime: bool,

    /// Accept required restart evidence.
    #[arg(long, requires = "require_runtime")]
    pub accept_restart_required: bool,

    /// Request a remote push after apply.
    #[arg(long)]
    pub push_remote: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PlanUseArgs {
    /// Skill id.
    pub skill: String,

    /// Target agents, comma-separated.
    #[arg(long, value_enum, value_delimiter = ',')]
    pub agents: Vec<AgentKind>,

    /// Target scope.
    #[arg(long, value_enum, default_value_t = UseScope::Project)]
    pub scope: UseScope,

    /// Project workspace (default: current directory).
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Binding profile.
    #[arg(long, default_value = "default")]
    pub profile: String,

    /// Projection method.
    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,

    /// Managed target root (default: <root>/targets/<scope>).
    #[arg(long)]
    pub target_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ApplyArgs {
    /// Plan id from `loom plan`.
    pub plan_id: String,

    /// Required reviewed digest for convergence plans.
    #[arg(long)]
    pub plan_digest: Option<String>,

    /// Retry-safe idempotency key.
    #[arg(long)]
    pub idempotency_key: String,

    /// Plan approvals; repeat or comma-separate.
    #[arg(long = "approve", value_delimiter = ',')]
    pub approvals: Vec<String>,
}
