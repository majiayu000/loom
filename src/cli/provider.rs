use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum ProviderCommand {
    #[command(about = "Add a skill catalog provider")]
    Add(ProviderAddArgs),
    #[command(about = "List configured and built-in providers")]
    List,
    #[command(about = "Remove a persisted provider")]
    Remove(ProviderRemoveArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProviderAddArgs {
    /// Provider id used as the locator prefix.
    pub id: String,

    /// Provider implementation kind.
    #[arg(long, value_enum)]
    pub kind: ProviderKindArg,

    /// Credential-free provider base URL or local catalog path.
    #[arg(long)]
    pub url: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProviderRemoveArgs {
    /// Persisted provider id to remove.
    pub id: String,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKindArg {
    Github,
    Local,
}
