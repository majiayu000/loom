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
    Migrate {
        #[command(subcommand)]
        command: MigrateCommand,
    },
    Panel(PanelArgs),

    // v1 commands are intentionally unsupported in v2.
    #[command(name = "init", hide = true)]
    LegacyInit(InitArgs),
    #[command(name = "add", hide = true)]
    LegacyAdd(AddArgs),
    #[command(name = "import", hide = true)]
    LegacyImport(ImportArgs),
    #[command(name = "link", hide = true)]
    LegacyLink(LinkArgs),
    #[command(name = "use", hide = true)]
    LegacyUse(LinkArgs),
    #[command(name = "save", hide = true)]
    LegacySave(SaveArgs),
    #[command(name = "snapshot", hide = true)]
    LegacySnapshot(SkillOnlyArgs),
    #[command(name = "release", hide = true)]
    LegacyRelease(ReleaseArgs),
    #[command(name = "rollback", hide = true)]
    LegacyRollback(RollbackArgs),
    #[command(name = "diff", hide = true)]
    LegacyDiff(DiffArgs),
    #[command(name = "status", hide = true)]
    LegacyStatus,
    #[command(name = "doctor", hide = true)]
    LegacyDoctor,
    #[command(name = "remote", hide = true)]
    LegacyRemote {
        #[command(subcommand)]
        command: RemoteCommand,
    },
}

#[derive(Debug, Clone, Subcommand)]
pub enum WorkspaceCommand {
    Init(InitArgs),
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
    Import(ImportArgs),
    Project(ProjectArgs),
    Capture(CaptureArgs),
    Link(LinkArgs),
    Use(LinkArgs),
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
pub enum MigrateCommand {
    #[command(name = "v2-to-v3")]
    V2ToV3(MigrateV2ToV3Args),
}

#[derive(Debug, Clone, Args)]
pub struct MigrateV2ToV3Args {
    #[arg(long, conflicts_with = "apply")]
    pub plan: bool,

    #[arg(long, conflicts_with = "plan")]
    pub apply: bool,
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
pub struct InitArgs {
    #[arg(long)]
    pub wizard: bool,

    #[arg(long, value_enum, default_value_t = Target::Both)]
    pub from_agent: Target,

    #[arg(long, value_enum, default_value_t = Target::Both)]
    pub target: Target,

    #[arg(long)]
    pub copy: bool,

    #[arg(long)]
    pub force: bool,

    #[arg(long)]
    pub skip_backup: bool,

    #[arg(long)]
    pub backup_dir: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    pub source: String,

    #[arg(long)]
    pub name: String,
}

#[derive(Debug, Clone, Args)]
pub struct ImportArgs {
    #[arg(long)]
    pub source: Option<String>,

    #[arg(long, value_enum)]
    pub from_agent: Option<Target>,

    #[arg(long)]
    pub skill: Option<String>,

    #[arg(long)]
    pub link: bool,

    #[arg(long, value_enum, default_value_t = Target::Both)]
    pub target: Target,

    #[arg(long)]
    pub copy: bool,

    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct LinkArgs {
    pub skill: String,

    #[arg(long, value_enum, default_value_t = Target::Both)]
    pub target: Target,

    #[arg(long)]
    pub copy: bool,
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
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
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

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Claude,
    Codex,
    Both,
}

#[cfg(test)]
mod tests {
    use super::WorkspaceMatcherKind;

    #[test]
    fn workspace_matcher_kind_deserializes_cli_and_api_spellings() {
        let kebab: WorkspaceMatcherKind =
            serde_json::from_str("\"path-prefix\"").expect("deserialize kebab-case matcher");
        let snake: WorkspaceMatcherKind =
            serde_json::from_str("\"path_prefix\"").expect("deserialize snake_case matcher");

        assert_eq!(kebab, WorkspaceMatcherKind::PathPrefix);
        assert_eq!(snake, WorkspaceMatcherKind::PathPrefix);
    }
}
