use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillsetCommand {
    #[command(about = "Create an empty skillset")]
    Create(SkillsetCreateArgs),
    #[command(about = "Add an existing skill to a skillset")]
    Add(SkillsetAddArgs),
    #[command(about = "Remove a skill from a skillset")]
    Remove(SkillsetMemberArgs),
    #[command(about = "Show a skillset with member summaries")]
    Show(SkillsetShowArgs),
    #[command(about = "Validate a skillset")]
    Lint(SkillsetShowArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetCreateArgs {
    /// Skillset id.
    pub name: String,

    /// Human-readable skillset description.
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetAddArgs {
    /// Skillset id.
    pub name: String,

    /// Existing registry skill id to add.
    pub skill: String,

    /// Optional member role, such as context, planning, execution, or handoff.
    #[arg(long)]
    pub role: Option<String>,

    /// Mark the member as required. This is the default.
    #[arg(long, conflicts_with = "optional")]
    pub required: bool,

    /// Mark the member as optional.
    #[arg(long, conflicts_with = "required")]
    pub optional: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetMemberArgs {
    /// Skillset id.
    pub name: String,

    /// Skill id to remove from the skillset.
    pub skill: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetShowArgs {
    /// Skillset id.
    pub name: String,
}
