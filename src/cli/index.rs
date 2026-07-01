use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct IndexArgs {
    /// Action to run: build or status.
    pub action: String,

    /// Skip embedding records even when a local provider is configured.
    #[arg(long)]
    pub no_embeddings: bool,

    /// Embedding provider. `local` falls back to no embeddings until configured.
    #[arg(long, default_value = "none")]
    pub provider: String,
}
