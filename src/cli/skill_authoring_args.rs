use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

use super::AgentKind;
use super::skill_new_args::SkillNewArgs;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillAuthorCommand {
    #[command(about = "Draft a new skill as a guarded patch artifact")]
    Draft(SkillDraftArgs),
    #[command(about = "Extract reviewed diff context into a guarded patch artifact")]
    Extract(SkillExtractArgs),
    #[command(about = "Rewrite one skill as a guarded patch artifact")]
    Rewrite(SkillRewriteArgs),
    #[command(about = "Tune one skill description as a guarded patch artifact")]
    TuneDescription(SkillTuneDescriptionArgs),
    #[command(about = "Generate reviewable eval fixture diffs as a patch artifact")]
    GenerateEvals(SkillGenerateEvalsArgs),
    #[command(about = "Apply a reviewed skill patch artifact through validation gates")]
    ApplyPatch(SkillApplyPatchArgs),
    #[command(about = "Create a lint-clean local skill skeleton")]
    New(SkillNewArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SkillAuthoringProviderArg {
    Mock,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillDraftArgs {
    /// New registry skill name to draft as a patch artifact.
    pub name: String,

    /// Explicit session file path or reviewed session id used as prompt material.
    #[arg(long)]
    pub from_session: String,

    /// Optional target agent used in the draft prompt.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Authoring provider. Only deterministic mock is enabled in this slice.
    #[arg(long, value_enum, default_value_t = SkillAuthoringProviderArg::Mock)]
    pub provider: SkillAuthoringProviderArg,

    /// Preview the patch artifact without writing state/patches files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillExtractArgs {
    /// Existing registry skill name.
    pub skill: String,

    /// Explicit diff file used as prompt material.
    #[arg(long)]
    pub from_diff: PathBuf,

    /// Authoring provider. Only deterministic mock is enabled in this slice.
    #[arg(long, value_enum, default_value_t = SkillAuthoringProviderArg::Mock)]
    pub provider: SkillAuthoringProviderArg,

    /// Preview the patch artifact without writing state/patches files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillRewriteArgs {
    /// Existing registry skill name.
    pub skill: String,

    /// Explicit rewrite goal for the provider prompt.
    #[arg(long)]
    pub instruction: String,

    /// Authoring provider. Only deterministic mock is enabled in this slice.
    #[arg(long, value_enum, default_value_t = SkillAuthoringProviderArg::Mock)]
    pub provider: SkillAuthoringProviderArg,

    /// Preview the patch artifact without writing state/patches files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillTuneDescriptionArgs {
    /// Existing registry skill name.
    pub skill: String,

    /// Replacement description to propose. Defaults to a deterministic mock tune.
    #[arg(long)]
    pub description: Option<String>,

    /// Authoring provider. Only deterministic mock is enabled in this slice.
    #[arg(long, value_enum, default_value_t = SkillAuthoringProviderArg::Mock)]
    pub provider: SkillAuthoringProviderArg,

    /// Preview the patch artifact without writing state/patches files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillGenerateEvalsArgs {
    /// Existing registry skill name.
    pub skill: String,

    /// Task description used to propose trigger/task fixtures.
    #[arg(long)]
    pub task: Option<String>,

    /// Authoring provider. Only deterministic mock is enabled in this slice.
    #[arg(long, value_enum, default_value_t = SkillAuthoringProviderArg::Mock)]
    pub provider: SkillAuthoringProviderArg,

    /// Preview the patch artifact without writing state/patches files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillApplyPatchArgs {
    /// Patch artifact id returned by an authoring command.
    pub patch_id: String,

    /// Idempotency key for guarded patch application.
    #[arg(long)]
    pub idempotency_key: Option<String>,
}
