use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use super::ProjectionMethod;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ActivationScope {
    User,
    Project,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillActivateArgs {
    pub skill: String,
    #[arg(long)]
    pub agent: String,
    #[arg(long, value_enum, default_value_t = ActivationScope::User)]
    pub scope: ActivationScope,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,
    #[arg(long)]
    pub compiled: bool,
    #[arg(long)]
    pub artifact: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillDeactivateArgs {
    pub skill: String,
    #[arg(long)]
    pub agent: String,
    #[arg(long, value_enum, default_value_t = ActivationScope::User)]
    pub scope: ActivationScope,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillActiveCommand {
    #[command(about = "List desired and realized active skills")]
    List(SkillActiveListArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillActiveListArgs {
    #[arg(long)]
    pub agent: String,
    #[arg(long, value_enum, default_value_t = ActivationScope::User)]
    pub scope: ActivationScope,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
}
