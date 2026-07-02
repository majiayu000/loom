use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use super::AgentKind;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum InstructionCommand {
    #[command(about = "Scan known non-skill instruction surfaces without mutation")]
    Scan(InstructionScanArgs),
    #[command(about = "Show one discovered instruction surface")]
    Show(InstructionShowArgs),
    #[command(about = "Classify one instruction file without mutation")]
    Classify(InstructionClassifyArgs),
    #[command(about = "Diagnose overlap between instructions and skills")]
    Doctor(InstructionDoctorArgs),
    #[command(about = "Plan an instruction migration without writing files")]
    MigratePlan(InstructionMigratePlanArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct InstructionScanArgs {
    /// Restrict discovery to one agent adapter.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Workspace path to scan. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct InstructionShowArgs {
    /// Instruction id returned by `instruction scan`.
    pub instruction_id: String,

    /// Workspace path that produced the instruction id. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct InstructionClassifyArgs {
    /// File path to classify.
    pub path: PathBuf,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct InstructionDoctorArgs {
    /// Restrict discovery to one agent adapter.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Workspace path to scan. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Compare discovered instructions against one registry skill.
    #[arg(long)]
    pub skill: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct InstructionMigratePlanArgs {
    /// Instruction id returned by `instruction scan`.
    pub instruction_id: String,

    /// Workspace path that produced the instruction id. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Review target for the dry-run migration plan.
    #[arg(long, value_enum)]
    pub to: InstructionMigrationTarget,

    /// Proposed skill name for skill/reference migration targets.
    #[arg(long)]
    pub name: Option<String>,

    /// Required. Instruction migration apply is deferred.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstructionMigrationTarget {
    Skill,
    Reference,
    KeepInstruction,
}
