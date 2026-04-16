use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(name = "loom")]
#[command(about = "Loom - Skill manager with Git-native backend")]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,

    #[arg(long, global = true)]
    pub request_id: Option<String>,

    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    Target {
        #[command(subcommand)]
        command: TargetCommand,
    },
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },
    Ops {
        #[command(subcommand)]
        command: OpsCommand,
    },
    Panel(PanelArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkspaceCommand {
    Status,
    Doctor,
    Binding {
        #[command(subcommand)]
        command: WorkspaceBindingCommand,
    },
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkspaceBindingCommand {
    Add(BindingAddArgs),
    List,
    Show(BindingShowArgs),
    Remove(BindingShowArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum TargetCommand {
    Add(TargetAddArgs),
    List,
    Show(TargetShowArgs),
    Remove(TargetShowArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum SkillCommand {
    Add(AddArgs),
    Project(ProjectArgs),
    Capture(CaptureArgs),
    Save(SaveArgs),
    Snapshot(SkillOnlyArgs),
    Release(ReleaseArgs),
    Rollback(RollbackArgs),
    Diff(DiffArgs),
}

#[derive(Debug, Clone, Subcommand)]
pub enum OpsCommand {
    List,
    Retry,
    Purge,
    History {
        #[command(subcommand)]
        command: OpsHistoryCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum OpsHistoryCommand {
    Diagnose,
    Repair(HistoryRepairArgs),
}

#[derive(Debug, Clone, Args)]
pub struct HistoryRepairArgs {
    #[arg(long, value_enum)]
    pub strategy: HistoryRepairStrategyArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HistoryRepairStrategyArg {
    Local,
    Remote,
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    pub source: String,

    #[arg(long)]
    pub name: String,
}

#[derive(Debug, Clone, Args)]
pub struct ProjectArgs {
    pub skill: String,

    #[arg(long)]
    pub binding: String,

    #[arg(long)]
    pub target: Option<String>,

    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,
}

#[derive(Debug, Clone, Args)]
pub struct CaptureArgs {
    pub skill: Option<String>,

    #[arg(long)]
    pub binding: Option<String>,

    #[arg(long)]
    pub instance: Option<String>,

    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct SaveArgs {
    pub skill: String,

    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct SkillOnlyArgs {
    pub skill: String,
}

#[derive(Debug, Clone, Args)]
pub struct ReleaseArgs {
    pub skill: String,
    pub version: String,
}

#[derive(Debug, Clone, Args)]
pub struct RollbackArgs {
    pub skill: String,

    #[arg(long)]
    pub to: Option<String>,

    #[arg(long)]
    pub steps: Option<u32>,
}

#[derive(Debug, Clone, Args)]
pub struct DiffArgs {
    pub skill: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Args)]
pub struct PanelArgs {
    #[arg(long, default_value_t = 43117)]
    pub port: u16,
}

#[derive(Debug, Clone, Args)]
pub struct BindingShowArgs {
    pub binding_id: String,
}

#[derive(Debug, Clone, Args)]
pub struct BindingAddArgs {
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    #[arg(long)]
    pub profile: String,

    #[arg(long, value_enum)]
    pub matcher_kind: WorkspaceMatcherKind,

    #[arg(long)]
    pub matcher_value: String,

    #[arg(long)]
    pub target: String,

    #[arg(long, default_value = "safe-capture")]
    pub policy_profile: String,
}

#[derive(Debug, Clone, Args)]
pub struct TargetShowArgs {
    pub target_id: String,
}

#[derive(Debug, Clone, Args)]
pub struct TargetAddArgs {
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    #[arg(long)]
    pub path: String,

    #[arg(long, value_enum, default_value_t = TargetOwnership::Managed)]
    pub ownership: TargetOwnership,
}

#[derive(Debug, Clone, Subcommand)]
pub enum RemoteCommand {
    Set { url: String },
    Status,
}

#[derive(Debug, Clone, Subcommand)]
pub enum SyncCommand {
    Status,
    Push,
    Pull,
    Replay,
}

#[derive(
    Debug,
    Clone,
    Copy,
    ValueEnum,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
)]
#[serde(rename_all = "kebab-case")]
pub enum AgentKind {
    Claude,
    Codex,
    Cursor,
    Windsurf,
    Cline,
    Copilot,
    Aider,
    Opencode,
    GeminiCli,
    Goose,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMatcherKind {
    #[serde(alias = "path-prefix")]
    PathPrefix,
    #[serde(alias = "exact-path")]
    ExactPath,
    Name,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TargetOwnership {
    Managed,
    Observed,
    External,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionMethod {
    Symlink,
    Copy,
    Materialize,
}

#[cfg(test)]
mod tests {
    use super::{AgentKind, WorkspaceMatcherKind};

    #[test]
    fn workspace_matcher_kind_deserializes_cli_and_api_spellings() {
        let kebab: WorkspaceMatcherKind =
            serde_json::from_str("\"path-prefix\"").expect("deserialize kebab-case matcher");
        let snake: WorkspaceMatcherKind =
            serde_json::from_str("\"path_prefix\"").expect("deserialize snake_case matcher");

        assert_eq!(kebab, WorkspaceMatcherKind::PathPrefix);
        assert_eq!(snake, WorkspaceMatcherKind::PathPrefix);
    }

    #[test]
    fn agent_kind_serde_round_trip_uses_kebab_case() {
        // Existing single-word variants must keep their legacy lowercase spelling
        // (kebab-case == lowercase for single words, so persisted data is unaffected).
        for (variant, wire) in [
            (AgentKind::Claude, "\"claude\""),
            (AgentKind::Codex, "\"codex\""),
            (AgentKind::Cursor, "\"cursor\""),
            (AgentKind::Windsurf, "\"windsurf\""),
            (AgentKind::Cline, "\"cline\""),
            (AgentKind::Copilot, "\"copilot\""),
            (AgentKind::Aider, "\"aider\""),
            (AgentKind::Opencode, "\"opencode\""),
            (AgentKind::Goose, "\"goose\""),
            // Multi-word variant uses kebab-case, matching the CLI flag value.
            (AgentKind::GeminiCli, "\"gemini-cli\""),
        ] {
            let serialized = serde_json::to_string(&variant).expect("serialize AgentKind");
            assert_eq!(serialized, wire, "serialize {:?}", variant);

            let deserialized: AgentKind =
                serde_json::from_str(wire).expect("deserialize AgentKind");
            assert_eq!(deserialized, variant, "deserialize {}", wire);
        }
    }
}
