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

    /// Restrict results by source status such as present, missing, or non-compliant.
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

    /// Request local semantic retrieval. Falls back to lexical mode when no local provider exists.
    #[arg(long)]
    pub semantic: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillRecommendArgs {
    /// Task description to rank skills and skillsets for.
    pub task_description: String,

    /// Prefer skills compatible with this agent.
    #[arg(long)]
    pub agent: Option<String>,

    /// Boost skills whose binding matcher covers this workspace path.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Request local semantic retrieval. Falls back to lexical mode when no local provider exists.
    #[arg(long)]
    pub semantic: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ActiveRecommendArgs {
    /// Action to run: recommend.
    pub action: String,

    /// Task description for the desired active state.
    pub task_description: String,

    /// Agent whose active view should be compared.
    #[arg(long)]
    pub agent: String,

    /// Workspace path for project-scoped recommendations.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Disambiguate the active binding when multiple bindings match.
    #[arg(long)]
    pub binding: Option<String>,

    /// Explicit desired skills to compare against the active view.
    #[arg(long = "desired-skill")]
    pub desired_skills: Vec<String>,
}
