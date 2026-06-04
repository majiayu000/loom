use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Parser, Serialize)]
#[command(name = "loom")]
#[command(version)]
#[command(about = "Loom - Skill manager with Git-native backend")]
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
    #[command(about = "Export, inspect, and restore portable registry backups")]
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
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
    #[command(
        about = "Plan safe agent automation before mutating state. Requires an existing workspace binding (`loom workspace binding add`) so preflight knows which target to project into."
    )]
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    #[command(about = "Serve the local registry control panel")]
    Panel(PanelArgs),
    #[command(
        about = "Run registry integrity, history, and projection checks (alias for `workspace doctor`)"
    )]
    Doctor,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum BackupCommand {
    #[command(about = "Create a portable registry backup artifact")]
    Export(BackupExportArgs),
    #[command(about = "Inspect and validate a registry backup artifact")]
    Inspect(BackupInspectArgs),
    #[command(about = "Restore a registry backup into a new empty root")]
    Restore(BackupRestoreArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BackupExportArgs {
    /// Output tar path. Defaults to <root>/backups/loom-backup-<timestamp>.tar.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Backup artifact format.
    #[arg(long, value_enum, default_value_t = BackupFormat::Tar)]
    pub format: BackupFormat,

    /// Include registry-owned target cache data if present.
    #[arg(long)]
    pub include_target_cache: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BackupInspectArgs {
    /// Backup artifact to inspect.
    pub artifact: PathBuf,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BackupRestoreArgs {
    /// Backup artifact to restore.
    pub artifact: PathBuf,

    /// Permit a destination root that contains only safe empty scaffolding.
    #[arg(long)]
    pub force_empty_root: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackupFormat {
    Tar,
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
    #[command(about = "Show Git history for one skill source")]
    History(HistoryArgs),
    #[command(about = "Move skills to trash, list trash entries, restore, or purge")]
    Trash {
        #[command(subcommand)]
        command: SkillTrashCommand,
    },
    #[command(about = "Verify a skill source has no uncommitted drift")]
    Verify(SkillOnlyArgs),
    #[command(about = "Diagnose one skill source and registry projection state")]
    Diagnose(SkillOnlyArgs),
    #[command(about = "Watch registry skill sources and autosave stable local edits")]
    Watch(WatchArgs),
    #[command(about = "Continuously import and update skills from observed targets")]
    MonitorObserved(MonitorObservedArgs),
    #[command(about = "Run one import pass over observed targets and exit")]
    ImportObserved(ImportObservedArgs),
    #[command(about = "Inspect and clean projections orphaned by binding removal")]
    Orphan {
        #[command(subcommand)]
        command: SkillOrphanCommand,
    },
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillTrashCommand {
    #[command(about = "Move a registry skill into Git-tracked trash")]
    Add(SkillOnlyArgs),
    #[command(about = "List Git-tracked trash entries")]
    List,
    #[command(about = "Restore a skill from trash")]
    Restore(TrashRestoreArgs),
    #[command(about = "Permanently remove one trash entry")]
    Purge(TrashPurgeArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TrashRestoreArgs {
    /// Registry skill name.
    pub skill: String,

    /// Restore a specific trash entry instead of the newest entry for the skill.
    #[arg(long)]
    pub trash_id: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TrashPurgeArgs {
    /// Trash entry id returned by `loom skill trash list`.
    pub trash_id: String,
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum SkillOrphanCommand {
    #[command(about = "List orphaned projection records")]
    List,
    #[command(about = "Remove orphaned projection records (and optionally their live files)")]
    Clean(OrphanCleanArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct OrphanCleanArgs {
    /// Also delete validated live projection directories.
    #[arg(long)]
    pub delete_live_paths: bool,

    /// Show the cleanup plan without modifying registry state or live files.
    #[arg(long)]
    pub dry_run: bool,
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

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum AgentCommand {
    #[command(about = "Resolve selectors and risks for an agent workspace")]
    Preflight(AgentPreflightArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct AgentPreflightArgs {
    /// Agent kind asking for the plan.
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    /// Workspace path the agent is operating in.
    #[arg(long)]
    pub workspace: PathBuf,

    /// Optional skill to resolve project/capture selectors for.
    #[arg(long)]
    pub skill: Option<String>,

    /// Desired projection method for a new project operation.
    #[arg(long, value_enum, default_value_t = ProjectionMethod::Symlink)]
    pub method: ProjectionMethod,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct HistoryRepairArgs {
    /// Which side should win when repairing operation-history divergence.
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

    /// Show the projection plan without writing registry state or target files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct CaptureArgs {
    /// Registry skill name. Optional only when --instance uniquely identifies the projection.
    pub skill: Option<String>,

    /// Binding id for selecting a projection when --instance is not provided.
    #[arg(long)]
    pub binding: Option<String>,

    /// Projection instance id to capture from directly.
    #[arg(long)]
    pub instance: Option<String>,

    /// Git commit message for the captured source revision.
    #[arg(long)]
    pub message: Option<String>,

    /// Show the capture plan without writing registry state or source files.
    #[arg(long)]
    pub dry_run: bool,
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
    /// Registry skill name.
    pub skill: String,

    /// Git commit message for the saved source revision.
    #[arg(long)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct WatchArgs {
    /// Registry skill name. Watches all registry skills when omitted.
    pub skill: Option<String>,

    /// Milliseconds changes must remain quiet before autosave.
    #[arg(long, default_value_t = 3000)]
    pub debounce_ms: u64,

    /// Maximum changed paths allowed in one autosave batch.
    #[arg(long, default_value_t = 20)]
    pub max_batch: usize,

    /// Print the autosave plan without committing.
    #[arg(long)]
    pub dry_run: bool,

    /// Run one scan and exit.
    #[arg(long)]
    pub once: bool,

    /// Stop after N scans. Mainly useful for supervised smoke tests.
    #[arg(long, hide = true)]
    pub max_cycles: Option<u64>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SkillOnlyArgs {
    /// Registry skill name.
    pub skill: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ReleaseArgs {
    /// Registry skill name.
    pub skill: String,
    /// Release tag or version label to create.
    pub version: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct RollbackArgs {
    /// Registry skill name.
    pub skill: String,

    /// Git revision or snapshot reference to restore from.
    #[arg(long)]
    pub to: Option<String>,

    /// Number of source commits to roll back when --to is not provided.
    #[arg(long)]
    pub steps: Option<u32>,

    /// Preview rollback impact without changing Git refs, source files, or registry state.
    #[arg(long = "preview", visible_alias = "dry-run")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct DiffArgs {
    /// Registry skill name.
    pub skill: String,
    /// Older revision, snapshot, or release reference.
    pub from: String,
    /// Newer revision, snapshot, or release reference.
    pub to: String,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct HistoryArgs {
    /// Registry skill name.
    pub skill: String,

    /// Maximum number of history entries to return.
    #[arg(long, default_value_t = 30)]
    pub limit: usize,

    /// Older revision boundary. When set, history uses <from>..<to>.
    #[arg(long)]
    pub from: Option<String>,

    /// Newer revision boundary.
    #[arg(long, default_value = "HEAD")]
    pub to: String,

    /// Include per-commit short diff statistics.
    #[arg(long)]
    pub include_diff_stat: bool,

    /// Include registry operations added by each history commit.
    #[arg(long, default_value_t = true)]
    pub include_ops: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct ImportObservedArgs {
    /// Restrict import to one observed target id.
    #[arg(long)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct PanelArgs {
    /// Local HTTP port for the registry control panel.
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
    #[arg(long, value_enum, default_value_t = TargetOwnership::Observed)]
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
    Push(SyncPushArgs),
    #[command(about = "Pull registry state and operation history")]
    Pull,
    #[command(about = "Replay pending operations")]
    Replay,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SyncPushArgs {
    /// Show the push plan without committing, pushing, or clearing pending ops.
    #[arg(long)]
    pub dry_run: bool,
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
