use clap::{Args, Subcommand};
use serde::Serialize;

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillCompileArgs {
    /// Registry skill name to compile. Use --skill for names that
    /// collide with nested commands such as list or verify.
    pub skill: Option<String>,

    /// Registry skill name to compile when the positional grammar is
    /// ambiguous.
    #[arg(long = "skill")]
    pub skill_selector: Option<String>,

    /// Return the compile plan without writing artifact files.
    #[arg(long)]
    pub dry_run: bool,

    /// Target agent for the planned artifact. Defaults to portable.
    #[arg(long, default_value = "portable")]
    pub agent: String,

    /// Target agent profile for the planned artifact.
    #[arg(long, default_value = "default")]
    pub profile: String,

    #[command(subcommand)]
    pub command: Option<SkillCompileCommand>,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillCompileCommand {
    #[command(about = "List known compiled artifacts for one skill without mutation")]
    List(SkillCompileListArgs),
    #[command(about = "Verify compiled artifact manifests, sidecars, digests, and gates")]
    Verify(SkillCompileVerifyArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillCompileListArgs {
    /// Registry skill name.
    pub skill: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillCompileVerifyArgs {
    /// Registry skill name.
    pub skill: String,

    /// Verify one artifact id instead of every artifact for the skill.
    #[arg(long)]
    pub artifact: Option<String>,
}
