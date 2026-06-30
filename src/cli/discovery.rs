use std::path::PathBuf;

use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillSearchArgs {
    /// Lexical query matched against skill id, description, tags, and warnings.
    pub query: String,

    /// Restrict results to skills compatible with this agent.
    #[arg(long)]
    pub agent: Option<String>,

    /// Restrict results to skills connected to this profile id.
    #[arg(long)]
    pub profile: Option<String>,

    /// Restrict results to a source status such as present, missing, or non-compliant.
    #[arg(long)]
    pub status: Option<String>,

    /// Restrict results by trust metadata. Only unknown is available until policy metadata lands.
    #[arg(long)]
    pub trust: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillResolveArgs {
    /// Task description to resolve deterministically against local skill metadata.
    pub task_description: String,

    /// Prefer skills compatible with this agent.
    #[arg(long)]
    pub agent: Option<String>,

    /// Boost skills whose binding matcher covers this workspace path.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}
