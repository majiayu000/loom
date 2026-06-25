use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

use super::SkillOnlyArgs;

#[derive(Debug, Clone, Args, Serialize)]
pub struct AddArgs {
    /// Local skill directory, Git URL, local Git repo, or github:owner/repo//subdir source.
    pub source: String,

    /// Registry skill name, e.g. rust-review.
    #[arg(long)]
    pub name: String,

    /// Source ref to import for Git/GitHub sources. May be a branch, tag, or commit.
    #[arg(long = "ref")]
    pub source_ref: Option<String>,

    /// Subdirectory inside the source repository or local path.
    #[arg(long)]
    pub subdir: Option<PathBuf>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillProvenanceCommand {
    #[command(about = "Inspect recorded source provenance and lock metadata")]
    Inspect(SkillOnlyArgs),
    #[command(about = "Verify current source digest against provenance and loom.lock")]
    Verify(SkillOnlyArgs),
    #[command(about = "Refresh provenance and loom.lock from the current skill source")]
    Refresh(SkillOnlyArgs),
}
