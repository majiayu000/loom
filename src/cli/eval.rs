use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillEvalArgs {
    #[command(subcommand)]
    pub command: Option<SkillEvalCommand>,

    /// Registry skill name for the legacy offline form.
    pub skill: Option<String>,

    /// Evaluate for one agent id.
    #[arg(long)]
    pub agent: Option<String>,

    /// Evaluate the same fixtures across a comma-separated agent matrix.
    #[arg(long, value_delimiter = ',')]
    pub matrix: Vec<String>,

    /// Optional model label to stamp into the eval report.
    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillEvalCommand {
    #[command(about = "Run offline trigger, task, and artifact fixture checks")]
    Offline(SkillEvalOfflineArgs),
    #[command(about = "Run with-skill versus no-skill task evals through an explicit runner")]
    Run(SkillEvalRunArgs),
    #[command(about = "Evaluate skill trigger precision and recall")]
    Trigger(SkillEvalTriggerArgs),
    #[command(about = "Compare two skill source refs through an explicit runner")]
    Compare(SkillEvalCompareArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillEvalOfflineArgs {
    /// Registry skill name.
    pub skill: String,

    /// Evaluate for one agent id.
    #[arg(long)]
    pub agent: Option<String>,

    /// Evaluate the same fixtures across a comma-separated agent matrix.
    #[arg(long, value_delimiter = ',')]
    pub matrix: Vec<String>,

    /// Optional model label to stamp into the eval report.
    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillEvalRunArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent id to evaluate.
    #[arg(long)]
    pub agent: String,

    /// Baseline to compare against.
    #[arg(long, value_enum)]
    pub baseline: EvalBaselineArg,

    /// Optional fixture workspace copied into isolated temp workspaces.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Task cases JSONL path. Relative paths resolve from the skill source.
    #[arg(long)]
    pub cases: Option<PathBuf>,

    /// Number of repeated runs per case.
    #[arg(long, default_value_t = 1)]
    pub runs: u32,

    /// Eval runner. Real agent runners are explicit opt-in.
    #[arg(long, value_enum, default_value_t = EvalRunnerArg::Mock)]
    pub runner: EvalRunnerArg,

    /// Print the execution plan without running agents, writing reports, or mutating workspaces.
    #[arg(long)]
    pub dry_run: bool,

    /// Report output path. Defaults under state/registry/evals/<skill>/.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillEvalTriggerArgs {
    /// Registry skill name.
    pub skill: String,

    /// Agent id to evaluate.
    #[arg(long)]
    pub agent: String,

    /// Trigger cases JSONL path. Relative paths resolve from the skill source.
    #[arg(long)]
    pub cases: Option<PathBuf>,

    /// Number of repeated trigger runs.
    #[arg(long, default_value_t = 1)]
    pub runs: u32,

    /// Eval runner. Real agent runners are explicit opt-in.
    #[arg(long, value_enum, default_value_t = EvalRunnerArg::Mock)]
    pub runner: EvalRunnerArg,

    /// Report output path. Defaults under state/registry/evals/<skill>/.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillEvalCompareArgs {
    /// Registry skill name.
    pub skill: String,

    /// Source ref used as the comparison base.
    #[arg(long = "from")]
    pub from_ref: String,

    /// Source ref or working-tree used as the comparison target.
    #[arg(long = "to")]
    pub to_ref: String,

    /// Agent id to evaluate.
    #[arg(long)]
    pub agent: String,

    /// Task cases JSONL path. Relative paths resolve from the skill source.
    #[arg(long)]
    pub cases: Option<PathBuf>,

    /// Eval runner. Real agent runners are explicit opt-in.
    #[arg(long, value_enum, default_value_t = EvalRunnerArg::Mock)]
    pub runner: EvalRunnerArg,

    /// Report output path. Defaults under state/registry/evals/<skill>/.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EvalBaselineArg {
    NoSkill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EvalRunnerArg {
    Mock,
    CodexCli,
}
