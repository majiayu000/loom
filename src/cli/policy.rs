use clap::Args;
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillPolicyArgs {
    /// Registry skill name.
    pub skill: String,

    /// Policy profile to evaluate. Built-ins: safe-capture, audit-only, deny-risky, strict.
    #[arg(long)]
    pub policy_profile: Option<String>,
}
