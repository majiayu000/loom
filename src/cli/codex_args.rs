use std::path::PathBuf;

use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum CodexCommand {
    #[command(about = "Plan or repair Codex active-view projection visibility")]
    Reconcile(CodexReconcileArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct CodexReconcileArgs {
    /// Preview active-view repairs without mutating registry, target, or Codex config.
    #[arg(long)]
    pub dry_run: bool,

    /// Apply safe Loom-owned projection repairs.
    #[arg(long)]
    pub apply: bool,

    /// Also repair safe active-skill disables in Codex config.
    #[arg(long)]
    pub fix_config: bool,

    /// Restrict planning to one workspace binding id.
    #[arg(long)]
    pub binding: Option<String>,

    /// Restrict planning to one Codex target id.
    #[arg(long)]
    pub target: Option<String>,

    /// Optional allowlist for future legacy cleanup flows.
    #[arg(long)]
    pub allowlist: Option<PathBuf>,
}
