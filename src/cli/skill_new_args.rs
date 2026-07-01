use clap::{Args, ValueEnum};
use serde::Serialize;

use super::AgentKind;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillNewArgs {
    /// Registry skill name to create under skills/<name>.
    pub name: String,

    /// Starter layout for the generated skill.
    #[arg(long, value_enum, default_value_t = SkillNewTemplate::Basic)]
    pub template: SkillNewTemplate,

    /// Frontmatter description. Defaults to a lint-clean editable placeholder.
    #[arg(long)]
    pub description: Option<String>,

    /// Optional target agent recorded in the Loom-local manifest.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Show generated paths and previews without writing files or Git state.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SkillNewTemplate {
    Basic,
    CodingWorkflow,
    Scripted,
    ReferenceHeavy,
}
