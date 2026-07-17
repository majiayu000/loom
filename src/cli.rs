use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

pub use crate::vocab::{
    AgentKind, MatcherKind as WorkspaceMatcherKind, Ownership as TargetOwnership, ProjectionMethod,
};
mod backup;
mod catalog;
mod codex_args;
mod deps;
mod discovery;
mod eval;
mod improve;
mod index;
mod instruction;
mod mcp;
mod package;
mod plan_flow;
mod policy;
mod provenance;
mod provider;
mod provision;
mod safety;
mod skill_activation_args;
mod skill_authoring_args;
mod skill_compile_args;
mod skill_inspect_args;
mod skill_lint_args;
mod skill_new_args;
mod skill_usage_args;
mod skill_visibility_args;
mod skillset;
mod telemetry;
mod use_flow;
mod version;
mod workflow;
pub use backup::{
    BackupCommand, BackupExportArgs, BackupFormat, BackupInspectArgs, BackupRestoreArgs,
};
pub use catalog::{
    CatalogCommand, CatalogPreviewArgs, CatalogSearchArgs, CatalogShowArgs, InstallTrustArg,
    SkillInstallArgs,
};
pub use codex_args::{CodexCommand, CodexReconcileArgs};
pub use deps::SkillDepsArgs;
pub use discovery::{ActiveRecommendArgs, SkillSearchArgs};
pub use eval::{
    EvalBaselineArg, EvalRunnerArg, SkillEvalArgs, SkillEvalCommand, SkillEvalCompareArgs,
    SkillEvalOfflineArgs, SkillEvalRunArgs, SkillEvalTriggerArgs,
};
pub use improve::{SkillImproveArgs, SkillRegressionArgs};
pub use index::IndexArgs;
pub use instruction::{
    InstructionClassifyArgs, InstructionCommand, InstructionDoctorArgs, InstructionMigratePlanArgs,
    InstructionMigrationTarget, InstructionScanArgs, InstructionShowArgs,
};
pub use mcp::{
    McpApplyArgs, McpCatalogCommand, McpCatalogSearchArgs, McpCatalogShowArgs, McpCommand,
    McpDoctorArgs, McpPlanArgs, McpRequirementCommand, McpRequirementListArgs,
};
pub use package::{
    PackageBuildArgs, PackageCommand, PackageFormatArg, PackagePlanArgs, PackageVerifyArgs,
};
pub use plan_flow::{ApplyArgs, PlanCommand, PlanConvergeArgs, PlanUseArgs};
pub use policy::{
    ApprovalCommand, ApprovalDecisionArgs, ApprovalListArgs, ApprovalRequestArgs,
    OrgPolicyCheckArgs, OrgPolicyCommand, OrgPolicyInitArgs, PolicyCommand, RoleGrantArgs,
    RolesCommand, SkillPolicyArgs,
};
pub use provenance::{AddArgs, SkillProvenanceCommand, SkillProvenanceOutdatedArgs};
pub use provider::{ProviderAddArgs, ProviderCommand, ProviderKindArg, ProviderRemoveArgs};
pub use provision::{
    ProvisionApplyArgs, ProvisionCommand, ProvisionDoctorArgs, ProvisionExportArgs,
    ProvisionExportFormatArg, ProvisionImportArgs, ProvisionPlanArgs, ProvisionTargetArg,
};
pub use safety::{SkillQuarantineArgs, SkillScanArgs, SkillTrustArgs};
pub use skill_activation_args::{
    ActivationScope, SkillActivateArgs, SkillActiveCommand, SkillActiveListArgs,
    SkillDeactivateArgs,
};
pub use skill_authoring_args::{
    SkillApplyPatchArgs, SkillAuthorCommand, SkillAuthoringProviderArg, SkillDraftArgs,
    SkillExtractArgs, SkillGenerateEvalsArgs, SkillRewriteArgs, SkillTuneDescriptionArgs,
};
pub use skill_compile_args::{
    SkillCompileArgs, SkillCompileCommand, SkillCompileListArgs, SkillCompileVerifyArgs,
};
pub use skill_inspect_args::SkillInspectArgs;
pub use skill_lint_args::SkillLintArgs;
pub use skill_new_args::{SkillNewArgs, SkillNewTemplate};
pub use skill_usage_args::{SkillFeedbackArgs, SkillUsedArgs};
pub use skill_visibility_args::{SkillDiagnoseArgs, SkillDiagnoseCheck, SkillVisibilityArgs};
pub use skillset::{
    SkillsetActivateArgs, SkillsetAddArgs, SkillsetCommand, SkillsetCreateArgs, SkillsetEvalArgs,
    SkillsetEvalBaselineArg, SkillsetMemberArgs, SkillsetReleaseArgs, SkillsetRollbackArgs,
    SkillsetShowArgs,
};
pub use telemetry::{
    TelemetryCommand, TelemetryEnableArgs, TelemetryExportArgs, TelemetryExportFormat,
    TelemetryIngestAgent, TelemetryIngestArgs, TelemetryPurgeArgs, TelemetryReportArgs,
};
pub use use_flow::{UseArgs, UseScope};
pub use version::{DiffArgs, HistoryArgs, ReleaseArgs, RollbackArgs, SkillCommitArgs};
pub use workflow::{
    WorkflowCommand, WorkflowCreateArgs, WorkflowPlanArgs, WorkflowPreflightArgs, WorkflowRunArgs,
    WorkflowShowArgs,
};

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
    #[command(about = "Plan or apply a human-friendly skill use flow")]
    Use(UseArgs),
    #[command(about = "Create durable, audited agent plans")]
    Plan {
        #[command(subcommand)]
        command: PlanCommand,
    },
    #[command(about = "Apply a durable agent plan with an idempotency key")]
    Apply(ApplyArgs),
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
    #[command(about = "Manage groups of registry skills")]
    Skillset {
        #[command(subcommand)]
        command: SkillsetCommand,
    },
    #[command(about = "Manage local privacy-preserving telemetry and analytics")]
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommand,
    },
    #[command(about = "Manage skill catalog providers")]
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    #[command(about = "Search and preview skill catalogs")]
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    #[command(about = "Plan, build, and verify portable skill packages")]
    Package {
        #[command(subcommand)]
        command: PackageCommand,
    },
    #[command(about = "Plan and apply guarded MCP server provisioning")]
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
    #[command(about = "Plan remote and devcontainer skill provisioning without mutation")]
    Provision {
        #[command(subcommand)]
        command: ProvisionCommand,
    },
    #[command(about = "Manage Git-backed org policy checks")]
    Policy {
        #[command(subcommand)]
        command: PolicyCommand,
    },
    #[command(about = "Request and decide org policy approvals")]
    Approval {
        #[command(subcommand)]
        command: ApprovalCommand,
    },
    #[command(about = "Manage org policy role grants")]
    Roles {
        #[command(subcommand)]
        command: RolesCommand,
    },
    #[command(about = "Inspect non-skill instruction surfaces without mutation")]
    Instruction {
        #[command(subcommand)]
        command: InstructionCommand,
    },
    #[command(about = "Plan and preflight guarded multi-skill workflows")]
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
    },
    #[command(about = "Build and inspect local derived recommendation indexes")]
    Index(IndexArgs),
    #[command(about = "Inspect and plan active-view changes")]
    Active(ActiveRecommendArgs),
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
    #[command(about = "Inspect and reconcile Codex active-view visibility")]
    Codex {
        #[command(subcommand)]
        command: CodexCommand,
    },
    #[command(about = "Serve the local registry control panel")]
    Panel(PanelArgs),
}

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum WorkspaceCommand {
    #[command(about = "Show registry status, targets, Git state, and operation backlog")]
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
    Remove(BindingRemoveArgs),
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
    #[command(about = "List registry and observed skills")]
    List,
    #[command(about = "Inspect one skill lifecycle status without mutating state")]
    Inspect(SkillInspectArgs),
    #[command(about = "Check one skill runtime dependencies and MCP readiness")]
    Deps(SkillDepsArgs),
    #[command(about = "Plan, write, list, and verify derived compiled runtime artifacts")]
    Compile(SkillCompileArgs),
    #[command(about = "Activate one skill for an agent target")]
    Activate(SkillActivateArgs),
    #[command(about = "Deactivate one skill for an agent target")]
    Deactivate(SkillDeactivateArgs),
    #[command(about = "List active skill desired state and projections")]
    Active {
        #[command(subcommand)]
        command: SkillActiveCommand,
    },
    #[command(about = "Search, resolve, and explain skills with deterministic scoring")]
    Search(SkillSearchArgs),
    #[command(about = "Recommend skills and skillsets for a task without mutating active views")]
    Recommend(SkillSearchArgs),
    #[command(about = "Resolve the best skill candidate for a task without mutating state")]
    Resolve(SkillSearchArgs),
    #[command(about = "Record skill telemetry")]
    Used(SkillUsedArgs),
    #[command(about = "Record recommendation feedback")]
    Feedback(SkillFeedbackArgs),
    #[command(about = "Create and apply guarded skill authoring artifacts")]
    Author {
        #[command(subcommand)]
        command: SkillAuthorCommand,
    },
    #[command(about = "Import a skill source into the registry")]
    Add(AddArgs),
    #[command(about = "Plan a provider-backed skill install")]
    Install(SkillInstallArgs),
    #[command(about = "Project a registry skill into a bound target")]
    Project(ProjectArgs),
    #[command(about = "Commit source changes from the registry or a live projection")]
    Commit(SkillCommitArgs),
    #[command(about = "Run a read-only single-skill improvement preflight")]
    Improve(SkillImproveArgs),
    #[command(about = "Compare one skill against a baseline for regressions")]
    Regression(SkillRegressionArgs),
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
    #[command(about = "Inspect, verify, and refresh skill source provenance")]
    Provenance {
        #[command(subcommand)]
        command: SkillProvenanceCommand,
    },
    #[command(about = "Lint one skill for portable Agent Skills compliance")]
    Lint(SkillLintArgs),
    #[command(about = "Report skill capabilities, risks, and policy decision before projection")]
    Policy(SkillPolicyArgs),
    #[command(about = "Scan one skill for safety and trust risks")]
    Scan(SkillScanArgs),
    #[command(about = "Persist registry-owned trust metadata for one skill")]
    Trust(SkillTrustArgs),
    #[command(about = "Quarantine one skill without deleting its source")]
    Quarantine(SkillQuarantineArgs),
    #[command(about = "Clear quarantine state for one skill")]
    Unquarantine(SkillOnlyArgs),
    #[command(about = "Explain whether one skill is visible to an agent active view")]
    Visibility(SkillVisibilityArgs),
    #[command(about = "Diagnose one skill source and registry projection state")]
    Diagnose(SkillDiagnoseArgs),
    #[command(about = "Run offline skill eval fixtures for trigger, task, and artifact checks")]
    Eval(SkillEvalArgs),
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
    Add(TrashAddArgs),
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
pub struct TrashAddArgs {
    /// Registry skill name.
    pub skill: String,

    /// Show the trash plan without moving files, writing registry state, or committing.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TrashPurgeArgs {
    /// Trash entry id returned by `loom skill trash list`.
    pub trash_id: String,

    /// Show the purge plan without deleting files, writing registry state, or committing.
    #[arg(long)]
    pub dry_run: bool,
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
    #[command(about = "List replayable registry operations")]
    List,
    #[command(about = "Retry replayable registry operations")]
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
    #[command(about = "Plan active-view reconciliation for an agent without mutation")]
    Reconcile(AgentReconcileArgs),
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
pub struct AgentReconcileArgs {
    /// Agent kind to plan active-view reconciliation for.
    #[arg(long, value_enum)]
    pub agent: AgentKind,

    /// Preview active-view repairs without mutating registry or target state.
    #[arg(long)]
    pub dry_run: bool,

    /// Restrict planning to one workspace binding id.
    #[arg(long)]
    pub binding: Option<String>,

    /// Restrict planning to one target id.
    #[arg(long)]
    pub target: Option<String>,

    /// Optional allowlist for future legacy cleanup flows.
    #[arg(long)]
    pub allowlist: Option<PathBuf>,
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

    /// Run the skill improvement preflight before committing.
    #[arg(long)]
    pub preflight: bool,
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
pub struct BindingRemoveArgs {
    pub binding_id: String,

    /// Remove the binding and mark dependent projections orphaned.
    #[arg(long)]
    pub orphan_projections: bool,
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
    #[command(about = "Replay registry operations")]
    Replay,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct SyncPushArgs {
    /// Show the push plan without committing, pushing, or clearing the operation backlog.
    #[arg(long)]
    pub dry_run: bool,
}
