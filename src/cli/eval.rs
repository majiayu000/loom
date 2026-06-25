use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillEvalArgs {
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
