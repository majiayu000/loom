use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum ProvisionCommand {
    #[command(about = "Create a read-only remote provisioning plan")]
    Plan(ProvisionPlanArgs),
    #[command(about = "Apply a reviewed provisioning plan with explicit gates")]
    Apply(ProvisionApplyArgs),
    #[command(about = "Diagnose remote provisioning readiness without mutation")]
    Doctor(ProvisionDoctorArgs),
    #[command(about = "Export a reviewed provisioning plan into a portable artifact")]
    Export(ProvisionExportArgs),
    #[command(about = "Inspect a provisioning artifact before apply")]
    Import(ProvisionImportArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProvisionPlanArgs {
    #[arg(long, value_enum)]
    pub target: ProvisionTargetArg,

    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[arg(long, default_value = "codex")]
    pub agent: String,

    #[arg(long)]
    pub output_plan: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProvisionApplyArgs {
    pub plan: String,

    #[arg(long)]
    pub idempotency_key: String,

    #[arg(long = "approve")]
    pub approvals: Vec<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProvisionDoctorArgs {
    #[arg(long, value_enum)]
    pub target: ProvisionTargetArg,

    #[arg(long)]
    pub workspace: Option<PathBuf>,

    #[arg(long, default_value = "codex")]
    pub agent: String,

    #[arg(long)]
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProvisionExportArgs {
    pub plan: String,

    #[arg(long, value_enum)]
    pub format: ProvisionExportFormatArg,

    #[arg(long)]
    pub output: PathBuf,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProvisionImportArgs {
    pub artifact: PathBuf,

    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProvisionTargetArg {
    Devcontainer,
    Codespaces,
    Remote,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProvisionExportFormatArg {
    Devcontainer,
    Shell,
    Tar,
}
