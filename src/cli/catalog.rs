use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum CatalogCommand {
    #[command(about = "Search advisory skill catalog metadata")]
    Search(CatalogSearchArgs),
    #[command(about = "Show one normalized catalog locator")]
    Show(CatalogShowArgs),
    #[command(about = "Preview skill source content without executing code")]
    Preview(CatalogPreviewArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct CatalogSearchArgs {
    /// Search query.
    pub query: String,

    /// Provider id to search. Network providers require this plus --allow-network.
    #[arg(long)]
    pub provider: Option<String>,

    /// Permit network-backed provider search.
    #[arg(long)]
    pub allow_network: bool,

    /// Agent requesting advisory catalog metadata.
    #[arg(long)]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct CatalogShowArgs {
    /// Provider locator, e.g. github:owner/repo//skills/foo@<ref>.
    pub locator: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct CatalogPreviewArgs {
    /// Provider locator to preview.
    pub locator: String,

    /// Source ref to preview when not already present in the locator.
    #[arg(long = "ref")]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillInstallArgs {
    /// Provider locator to install from.
    pub locator: String,

    /// Registry skill name that would be written.
    #[arg(long)]
    pub name: String,

    /// Source ref to install when not already present in the locator.
    #[arg(long = "ref")]
    pub source_ref: Option<String>,

    /// Trust level to assign after review.
    #[arg(long, value_enum)]
    pub trust: Option<InstallTrustArg>,

    /// Evidence id required when --trust reviewed is used.
    #[arg(long)]
    pub review_evidence: Option<String>,

    /// Policy profile used for provider install evaluation.
    #[arg(long)]
    pub policy_profile: Option<String>,

    /// Show the install plan without writing registry state or skill files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstallTrustArg {
    ThirdPartyUnreviewed,
    Reviewed,
}
