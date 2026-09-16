use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

use super::AgentKind;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum WorkflowCommand {
    #[command(about = "Create a named workflow DAG definition")]
    Create(WorkflowCreateArgs),
    #[command(about = "Show a named workflow DAG definition")]
    Show(WorkflowShowArgs),
    #[command(about = "Create an auditable workflow plan without executing it")]
    Plan(WorkflowPlanArgs),
    #[command(about = "Revalidate a stored workflow plan")]
    Preflight(WorkflowPreflightArgs),
    #[command(about = "Execute a reviewed workflow plan with the configured Codex CLI")]
    Apply(WorkflowApplyArgs),
    #[command(
        hide = true,
        about = "Hidden deferred workflow execution compatibility surface"
    )]
    Run(WorkflowRunArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkflowCreateArgs {
    /// Workflow id to create.
    pub name: String,

    /// Workflow JSON definition to import.
    #[arg(long)]
    pub file: Option<PathBuf>,

    /// Materialize a dry-run workflow from a skillset.
    #[arg(long)]
    pub from_skillset: Option<String>,

    /// Preview the normalized workflow without writing registry state.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkflowShowArgs {
    /// Workflow id to inspect.
    pub name: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkflowPlanArgs {
    /// Workflow id to plan.
    pub workflow: String,

    /// Agent whose active view should be checked.
    #[arg(long)]
    pub agent: AgentKind,

    /// Workspace path for project-scoped active checks.
    #[arg(long)]
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkflowPreflightArgs {
    /// Workflow plan id returned by `workflow plan`.
    pub plan_id: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkflowApplyArgs {
    pub plan_id: String,
    #[arg(long)]
    pub idempotency_key: String,
    /// JSON file containing the workflow's named external inputs.
    #[arg(long)]
    pub inputs: Option<PathBuf>,
    /// Acknowledge the approval names displayed in the reviewed plan.
    #[arg(long, value_delimiter = ',')]
    pub approve: Vec<String>,
    /// Revalidate and preview without invoking Codex or changing workspace files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkflowRunArgs {
    /// Workflow id for the hidden deferred execution compatibility surface.
    pub name: String,

    /// Agent whose active view should be checked.
    #[arg(long)]
    pub agent: AgentKind,

    /// Workspace path for project-scoped active checks.
    #[arg(long)]
    pub workspace: PathBuf,

    /// Only report the deferred execution plan.
    #[arg(long)]
    pub dry_run: bool,
}
