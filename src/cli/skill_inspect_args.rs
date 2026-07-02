use std::path::PathBuf;

use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillInspectArgs {
    /// Registry skill name.
    pub skill: String,

    /// Focus runtime output on one agent id.
    #[arg(long)]
    pub agent: Option<String>,

    /// Workspace path used to select matching bindings.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Profile id used to select matching bindings.
    #[arg(long)]
    pub profile: Option<String>,

    /// Include local telemetry usage and eval summary when telemetry state exists.
    #[arg(long)]
    pub include_telemetry: bool,
}
