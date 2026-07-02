use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum McpCommand {
    #[command(about = "List MCP requirements declared by a skill")]
    Requirement {
        #[command(subcommand)]
        command: McpRequirementCommand,
    },
    #[command(about = "Create a read-only MCP provisioning plan")]
    Plan(McpPlanArgs),
    #[command(about = "Diagnose MCP provisioning readiness without mutation")]
    Doctor(McpDoctorArgs),
    #[command(about = "Search and inspect known MCP server catalog entries")]
    Catalog {
        #[command(subcommand)]
        command: McpCatalogCommand,
    },
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum McpRequirementCommand {
    #[command(about = "List requirements for one skill")]
    List(McpRequirementListArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct McpRequirementListArgs {
    #[arg(long)]
    pub skill: String,

    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct McpPlanArgs {
    #[arg(long)]
    pub skill: String,

    #[arg(long)]
    pub agent: String,

    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct McpDoctorArgs {
    #[arg(long)]
    pub agent: String,

    #[arg(long)]
    pub skill: Option<String>,

    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum McpCatalogCommand {
    #[command(about = "Search known MCP server catalog entries")]
    Search(McpCatalogSearchArgs),
    #[command(about = "Show one known MCP server catalog entry")]
    Show(McpCatalogShowArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct McpCatalogSearchArgs {
    pub query: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct McpCatalogShowArgs {
    pub server: String,
}
