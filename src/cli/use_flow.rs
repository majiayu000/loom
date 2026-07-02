use std::path::PathBuf;

use clap::{Args, ValueEnum};
use serde::{Deserialize, Serialize};

use super::{AgentKind, ProjectionMethod};

#[derive(Debug, Clone, Args, Deserialize, Serialize)]
pub struct UseArgs {
    /// Registry skill id to use.
    pub skill: String,

    /// Comma-separated agents to prepare, for example claude,codex.
    #[arg(long, value_enum, value_delimiter = ',')]
    pub agents: Vec<AgentKind>,

    /// Scope for target and binding creation.
    #[arg(long, value_enum, default_value_t = UseScope::Project)]
    pub scope: UseScope,

    /// Workspace path used for project scope matching. Defaults to current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Profile label for the generated or reused binding.
    #[arg(long, default_value = "default")]
    pub profile: String,

    /// Projection method to use when applying.
    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,

    /// Target skills directory. Defaults to the adapter discovery root for the selected scope.
    #[arg(long)]
    pub target_root: Option<PathBuf>,

    /// Adopt an existing agent skills directory as a managed Loom target before writing.
    #[arg(long)]
    pub adopt: bool,

    /// Apply the plan. Without this flag, `loom use` is read-only.
    #[arg(long)]
    pub apply: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UseScope {
    User,
    Project,
}
