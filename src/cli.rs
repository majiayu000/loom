use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Parser, Serialize)]
#[command(name = "loom")]
#[command(about = "Loom - Skill manager with Git-native backend")]
#[command(version)]
pub struct Cli {
    /// Print a stable machine-readable JSON envelope.
    #[arg(long, global = true)]
    pub json: bool,

    /// Pretty-print the JSON envelope. Ignored unless --json is set.
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Correlate this command with an external automation request.
    #[arg(long, global = true)]
    pub request_id: Option<String>,

    /// Registry root. Defaults to ~/.loom-registry.
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum Command {
    #[command(about = "Initialize the default registry and scan existing agent skill directories")]
    Init,
    #[command(about = "Import and update skills from observed targets")]
    Monitor(MonitorObservedArgs),
    #[command(about = "Inspect and configure registry workspace state")]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    #[command(about = "Register and inspect agent skill directories")]
    Target {
        #[command(subcommand)]
        command: TargetCommand,
    },
    #[command(about = "Manage skill sources, projections, and versions")]
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    #[command(about = "Synchronize the registry through its Git remote")]
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
    },
    #[command(about = "Inspect, replay, and repair operation history")]
    Ops {
        #[command(subcommand)]
        command: OpsCommand,
    },
    #[command(about = "Serve the local registry control panel")]
    Panel(PanelArgs),
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum WorkspaceCommand {
    #[command(about = "Show registry status, targets, Git state, and pending ops")]
    Status,
    #[command(about = "Run registry integrity, history, and projection checks")]
    Doctor,
    #[command(about = "Initialize registry state")]
    Init(WorkspaceInitArgs),
    #[command(about = "Manage workspace-to-target bindings")]
    Binding {
        #[command(subcommand)]
        command: WorkspaceBindingCommand,
    },
    #[command(about = "Configure and inspect the registry Git remote")]
    Remote {
        #[command(subcommand)]
        command: RemoteCommand,
    },
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WorkspaceInitArgs {
    /// Also scan default agent skill directories (~/.claude/skills,
    /// ~/.codex/skills) and auto-register any that exist as observed
    /// targets. Safe to re-run: existing targets are not duplicated.
    #[arg(long)]
    pub scan_existing: bool,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum WorkspaceBindingCommand {
    #[command(about = "Create a binding from a workspace matcher to a target")]
    Add(BindingAddArgs),
    #[command(about = "List workspace bindings")]
    List,
    #[command(about = "Show one binding with rules and projections")]
    Show(BindingShowArgs),
    #[command(about = "Remove a workspace binding")]
    Remove(BindingShowArgs),
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum TargetCommand {
    #[command(about = "Register an agent skill directory as a target")]
    Add(TargetAddArgs),
    #[command(about = "List registered projection targets")]
    List,
    #[command(about = "Show one target with related bindings and projections")]
    Show(TargetShowArgs),
    #[command(about = "Remove a projection target")]
    Remove(TargetShowArgs),
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillCommand {
    #[command(about = "Import a skill source into the registry")]
    Add(AddArgs),
    #[command(about = "Project a registry skill into a bound target")]
    Project(ProjectArgs),
    #[command(about = "Capture live projection edits back to the source")]
    Capture(CaptureArgs),
    #[command(about = "Commit source changes for one skill")]
    Save(SaveArgs),
    #[command(about = "Create a version snapshot for one skill")]
    Snapshot(SkillOnlyArgs),
    #[command(about = "Tag a skill release")]
    Release(ReleaseArgs),
    #[command(about = "Roll back a skill source to an earlier revision")]
    Rollback(RollbackArgs),
    #[command(about = "Diff two revisions of a skill source")]
    Diff(DiffArgs),
    #[command(about = "Continuously import and update skills from observed targets")]
    MonitorObserved(MonitorObservedArgs),
    #[command(about = "Import discovered skills from observed targets in one pass")]
    ImportObserved(ImportObservedArgs),
    #[command(about = "Inspect and clean projections orphaned by binding removal")]
    Orphan {
        #[command(subcommand)]
        command: SkillOrphanCommand,
    },
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillOrphanCommand {
    #[command(about = "Remove orphaned projection records (and optionally their files)")]
    Clean(OrphanCleanArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct OrphanCleanArgs {
    /// Also delete validated live projection directories.
    #[arg(long)]
    pub delete_live_paths: bool,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum OpsCommand {
    #[command(about = "List pending operations")]
    List,
    #[command(about = "Retry pending operations")]
    Retry,
    #[command(about = "Purge completed operation records")]
    Purge,
    #[command(about = "Diagnose and repair the loom-history branch")]
    History {
        #[command(subcommand)]
        command: OpsHistoryCommand,
    },
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum OpsHistoryCommand {
    #[command(about = "Report local and remote operation-history health")]
    Diagnose,
    #[command(about = "Repair operation-history divergence")]
    Repair(HistoryRepairArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct HistoryRepairArgs {
    #[arg(long, value_enum)]
    pub strategy: HistoryRepairStrategyArg,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
pub enum HistoryRepairStrategyArg {
    Local,
    Remote,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct AddArgs {
    /// Local skill directory or Git URL to import.
    pub source: String,

    /// Registry skill name, e.g. rust-review.
    #[arg(long)]
    pub name: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ProjectArgs {
    /// Registry skill name.
    pub skill: String,

    /// Workspace binding id that selects the default target.
    #[arg(long)]
    pub binding: String,

    /// Optional target id override.
    #[arg(long)]
    pub target: Option<String>,

    /// Projection strategy used for the live agent directory.
    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct CaptureArgs {
    pub skill: Option<String>,

    #[arg(long)]
    pub binding: Option<String>,

    #[arg(long)]
    pub instance: Option<String>,

    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct MonitorObservedArgs {
    /// Restrict monitoring to one observed target id.
    #[arg(long)]
    pub target: Option<String>,

    /// Run one scan and exit.
    #[arg(long)]
    pub once: bool,

    /// Seconds between scans in long-running mode.
    #[arg(long, default_value_t = 30)]
    pub interval_seconds: u64,

    /// Stop after N scans. Mainly useful for supervised smoke tests.
    #[arg(long, hide = true)]
    pub max_cycles: Option<u64>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SaveArgs {
    pub skill: String,

    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillOnlyArgs {
    pub skill: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ReleaseArgs {
    pub skill: String,
    pub version: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct RollbackArgs {
    pub skill: String,

    #[arg(long)]
    pub to: Option<String>,

    #[arg(long)]
    pub steps: Option<u32>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct DiffArgs {
    pub skill: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ImportObservedArgs {
    #[arg(long)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PanelArgs {
    #[arg(long, default_value_t = 43117)]
    pub port: u16,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BindingShowArgs {
    pub binding_id: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BindingAddArgs {
    /// Agent kind for this workspace binding.
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    /// Profile label such as home, work, or repo.
    #[arg(long)]
    pub profile: String,

    /// Matcher type used to decide when this binding applies.
    #[arg(long, value_enum)]
    pub matcher_kind: WorkspaceMatcherKind,

    /// Matcher value, usually an absolute project path.
    #[arg(long)]
    pub matcher_value: String,

    /// Default target id for this binding.
    #[arg(long)]
    pub target: String,

    /// Policy profile controlling capture/projection behavior.
    #[arg(long, default_value = "safe-capture")]
    pub policy_profile: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TargetShowArgs {
    pub target_id: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TargetAddArgs {
    /// Agent kind that reads this skills directory.
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    /// Absolute path to an agent skills directory.
    #[arg(long)]
    pub path: String,

    /// Whether Loom can write to this target.
    #[arg(long, value_enum, default_value_t = TargetOwnership::Managed)]
    pub ownership: TargetOwnership,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum RemoteCommand {
    #[command(about = "Set the registry Git remote URL")]
    Set { url: String },
    #[command(about = "Show remote URL, tracking, and sync state")]
    Status,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SyncCommand {
    #[command(about = "Show Git sync state")]
    Status,
    #[command(about = "Push registry state and operation history")]
    Push,
    #[command(about = "Pull registry state and operation history")]
    Pull,
    #[command(about = "Replay pending operations")]
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
