use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillLintArgs {
    /// Registry skill name.
    pub skill: String,

    /// Require portable Agent Skills compliance. This is the default mode.
    #[arg(long, conflicts_with_all = ["portable", "compat", "fix"])]
    pub strict: bool,

    /// Alias for strict portable Agent Skills compliance.
    #[arg(long, conflicts_with_all = ["strict", "compat", "fix"])]
    pub portable: bool,

    /// Accept legacy compatibility while reporting typed warnings.
    #[arg(long, conflicts_with_all = ["strict", "portable", "fix"])]
    pub compat: bool,

    /// Return a read-only fix plan for safe normalizations.
    #[arg(long, conflicts_with_all = ["strict", "portable", "compat"])]
    pub fix: bool,

    /// Add target-agent compatibility checks such as codex or claude.
    #[arg(long)]
    pub agent: Option<String>,

    /// Add non-fatal maintainability and context-budget checks.
    #[arg(long)]
    pub quality: bool,
}
