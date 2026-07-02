use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum PackageCommand {
    #[command(about = "Plan a portable skill or skillset package without writing artifacts")]
    Plan(PackagePlanArgs),
    #[command(about = "Build a package artifact from a reviewed plan")]
    Build(PackageBuildArgs),
    #[command(about = "Verify a package artifact manifest, checksums, and content")]
    Verify(PackageVerifyArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PackagePlanArgs {
    /// Source selector: skill:<id>, skillset:<id>, or a bare id when unambiguous.
    pub source: String,

    /// Package format to plan.
    #[arg(long, value_enum)]
    pub format: PackageFormatArg,

    /// Optional target agent for future native format gates.
    #[arg(long)]
    pub agent: Option<String>,

    /// Optional reviewed plan artifact path for later package build.
    #[arg(long)]
    pub output_plan: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PackageBuildArgs {
    /// Plan id or explicit plan artifact path. This slice supports plan artifacts.
    pub plan: String,

    /// Output artifact path.
    #[arg(long)]
    pub output: PathBuf,

    /// Idempotency key for retry-safe package builds.
    #[arg(long)]
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PackageVerifyArgs {
    /// Artifact path to verify.
    pub artifact: PathBuf,

    /// Expected package format.
    #[arg(long, value_enum)]
    pub format: Option<PackageFormatArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
pub enum PackageFormatArg {
    AgentSkillsArchive,
    CodexPlugin,
    ClaudePlugin,
    Npm,
    GithubRelease,
}
