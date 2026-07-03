use std::path::PathBuf;

use clap::ValueEnum;
use clap::{Args, Subcommand};
use serde::Serialize;

use super::skill_activation_args::ActivationScope;

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
    #[command(about = "Activate every ready member in a skillset")]
    Activate(SkillsetActivateArgs),
    #[command(about = "Deactivate every member in a skillset")]
    Deactivate(SkillsetActivateArgs),
    #[command(about = "Aggregate member eval results for a skillset")]
    Eval(SkillsetEvalArgs),
    #[command(about = "Create a release tag for a skillset definition")]
    Release(SkillsetReleaseArgs),
    #[command(about = "Restore a skillset definition from a version or ref")]
    Rollback(SkillsetRollbackArgs),
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

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetActivateArgs {
    /// Skillset id.
    pub name: String,

    /// Agent id to activate for.
    #[arg(long)]
    pub agent: String,

    /// Activation scope.
    #[arg(long, value_enum, default_value_t = ActivationScope::User)]
    pub scope: ActivationScope,

    /// Workspace path for project-scope activation.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Agent profile id.
    #[arg(long)]
    pub profile: Option<String>,

    /// Print the activation/deactivation plan without mutating state or targets.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetEvalArgs {
    /// Skillset id.
    pub name: String,

    /// Agent id to evaluate.
    #[arg(long)]
    pub agent: String,

    /// Baseline to compare against.
    #[arg(long, value_enum, default_value_t = SkillsetEvalBaselineArg::NoSkill)]
    pub baseline: SkillsetEvalBaselineArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum SkillsetEvalBaselineArg {
    NoSkill,
    SingleSkills,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetReleaseArgs {
    /// Skillset id.
    pub name: String,

    /// Release version label.
    pub version: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillsetRollbackArgs {
    /// Skillset id.
    pub name: String,

    /// Release version or Git ref to restore from.
    #[arg(long)]
    pub to: String,
}
