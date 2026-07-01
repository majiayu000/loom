use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillScanArgs {
    /// Registry skill name.
    pub skill: String,

    /// Safety context to evaluate.
    #[arg(long, default_value = "activate")]
    pub mode: String,

    /// Treat high-risk findings as blocking.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillTrustArgs {
    /// Registry skill name.
    pub skill: String,

    /// Trust level to persist in registry metadata.
    #[arg(long)]
    pub level: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillQuarantineArgs {
    /// Registry skill name.
    pub skill: String,

    /// Human-readable reason for quarantine.
    #[arg(long)]
    pub reason: Option<String>,
}
