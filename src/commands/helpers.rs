use std::path::Path;

use anyhow::{Result, anyhow};
use uuid::Uuid;

use crate::cli::{
    AgentCommand, AgentKind, ApprovalCommand, BindingAddArgs, CodexCommand, Command,
    InstructionCommand, McpCatalogCommand, McpCommand, McpRequirementCommand, OpsCommand,
    OpsHistoryCommand, OrgPolicyCommand, PackageCommand, PlanCommand, PolicyCommand,
    ProjectionMethod, ProvisionCommand, RolesCommand, SkillActiveCommand, SkillCommand,
    SkillCompileCommand, SkillOrphanCommand, SkillTrashCommand, SkillsetCommand, SyncCommand,
    TargetCommand, TelemetryCommand, WorkflowCommand, WorkspaceBindingCommand, WorkspaceCommand,
    WorkspaceMatcherKind,
};
use crate::state::AppContext;
use crate::state_model::{
    RegistryProjectionTarget, RegistryTargetCapabilities, RegistryTargetsFile,
};
use crate::types::ErrorCode;

use super::CommandFailure;

// ---------------------------------------------------------------------------
// Git bootstrap
// ---------------------------------------------------------------------------

pub(crate) fn ensure_initial_commit(ctx: &AppContext) -> Result<()> {
    use crate::gitops;
    if gitops::head(ctx).is_ok() {
        return Ok(());
    }
    gitops::run_git(
        ctx,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "chore: initialize skill registry",
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Skill validation
// ---------------------------------------------------------------------------

pub(crate) fn ensure_skill_exists(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    if !ctx.skill_path(skill).exists() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    Ok(())
}

pub(crate) fn validate_skill_name(skill: &str) -> Result<()> {
    if skill.is_empty() {
        return Err(anyhow!("skill name cannot be empty"));
    }
    if skill == "." || skill == ".." {
        return Err(anyhow!("skill name cannot be '.' or '..'"));
    }
    if skill
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
    {
        return Err(anyhow!(
            "skill name '{}' contains unsupported characters; use [A-Za-z0-9._-]",
            skill
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Command name dispatch
// ---------------------------------------------------------------------------

pub(crate) fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Init => "init",
        Command::Backup { command } => match command {
            crate::cli::BackupCommand::Export(_) => "backup.export",
            crate::cli::BackupCommand::Inspect(_) => "backup.inspect",
            crate::cli::BackupCommand::Restore(_) => "backup.restore",
        },
        Command::Monitor(_) => "monitor",
        Command::Use(_) => "use",
        Command::Plan { command } => match command {
            PlanCommand::Use(_) => "plan.use",
        },
        Command::Apply(_) => "apply",
        Command::Workspace { command } => match command {
            WorkspaceCommand::Status => "workspace.status",
            WorkspaceCommand::Doctor => "workspace.doctor",
            WorkspaceCommand::Init(_) => "workspace.init",
            WorkspaceCommand::Binding { command } => match command {
                WorkspaceBindingCommand::Add(_) => "workspace.binding.add",
                WorkspaceBindingCommand::List => "workspace.binding.list",
                WorkspaceBindingCommand::Show(_) => "workspace.binding.show",
                WorkspaceBindingCommand::Remove(_) => "workspace.binding.remove",
            },
            WorkspaceCommand::Remote { .. } => "workspace.remote",
        },
        Command::Target { command } => match command {
            TargetCommand::Add(_) => "target.add",
            TargetCommand::List => "target.list",
            TargetCommand::Show(_) => "target.show",
            TargetCommand::Remove(_) => "target.remove",
        },
        Command::Skill { command } => match command {
            SkillCommand::List => "skill.list",
            SkillCommand::Inspect(_) => "skill.inspect",
            SkillCommand::Deps(_) => "skill.deps",
            SkillCommand::Compile(args) => match &args.command {
                Some(SkillCompileCommand::List(_)) => "skill.compile.list",
                Some(SkillCompileCommand::Verify(_)) => "skill.compile.verify",
                None if args.dry_run => "skill.compile.dry_run",
                None => "skill.compile",
            },
            SkillCommand::Activate(_) => "skill.activate",
            SkillCommand::Deactivate(_) => "skill.deactivate",
            SkillCommand::Active { command } => match command {
                SkillActiveCommand::List(_) => "skill.active.list",
            },
            SkillCommand::Search(_) => "skill.search",
            SkillCommand::Recommend(_) => "skill.recommend",
            SkillCommand::Resolve(_) => "skill.resolve",
            SkillCommand::Draft(_) => "skill.draft",
            SkillCommand::Extract(_) => "skill.extract",
            SkillCommand::Rewrite(_) => "skill.rewrite",
            SkillCommand::TuneDescription(_) => "skill.tune_description",
            SkillCommand::GenerateEvals(_) => "skill.generate_evals",
            SkillCommand::ApplyPatch(_) => "skill.apply_patch",
            SkillCommand::New(_) => "skill.new",
            SkillCommand::Add(_) => "skill.add",
            SkillCommand::Install(_) => "skill.install",
            SkillCommand::ImportObserved(_) => "skill.import_observed",
            SkillCommand::MonitorObserved(_) => "skill.monitor_observed",
            SkillCommand::Project(_) => "skill.project",
            SkillCommand::Commit(_) => "skill.commit",
            SkillCommand::Improve(_) => "skill.improve",
            SkillCommand::Regression(_) => "skill.regression",
            SkillCommand::Watch(_) => "skill.watch",
            SkillCommand::Release(_) => "skill.release",
            SkillCommand::Rollback(_) => "skill.rollback",
            SkillCommand::Diff(_) => "skill.diff",
            SkillCommand::History(_) => "skill.history",
            SkillCommand::Lint(_) => "skill.lint",
            SkillCommand::Policy(_) => "skill.policy",
            SkillCommand::Scan(_) => "skill.scan",
            SkillCommand::Trust(_) => "skill.trust",
            SkillCommand::Quarantine(_) => "skill.quarantine",
            SkillCommand::Unquarantine(_) => "skill.unquarantine",
            SkillCommand::Visibility(_) => "skill.visibility",
            SkillCommand::Diagnose(_) => "skill.diagnose",
            SkillCommand::Eval(_) => "skill.eval",
            SkillCommand::Provenance { command } => match command {
                crate::cli::SkillProvenanceCommand::Inspect(_) => "skill.provenance.inspect",
                crate::cli::SkillProvenanceCommand::Verify(_) => "skill.provenance.verify",
                crate::cli::SkillProvenanceCommand::Refresh(_) => "skill.provenance.refresh",
            },
            SkillCommand::Trash {
                command: SkillTrashCommand::Add(_),
            } => "skill.trash.add",
            SkillCommand::Trash {
                command: SkillTrashCommand::List,
            } => "skill.trash.list",
            SkillCommand::Trash {
                command: SkillTrashCommand::Restore(_),
            } => "skill.trash.restore",
            SkillCommand::Trash {
                command: SkillTrashCommand::Purge(_),
            } => "skill.trash.purge",
            SkillCommand::Orphan {
                command: SkillOrphanCommand::List,
            } => "skill.orphan.list",
            SkillCommand::Orphan {
                command: SkillOrphanCommand::Clean(_),
            } => "skill.orphan.clean",
        },
        Command::Skillset { command } => match command {
            SkillsetCommand::Create(_) => "skillset.create",
            SkillsetCommand::Add(_) => "skillset.add",
            SkillsetCommand::Remove(_) => "skillset.remove",
            SkillsetCommand::Show(_) => "skillset.show",
            SkillsetCommand::Lint(_) => "skillset.lint",
            SkillsetCommand::Activate(_) => "skillset.activate",
            SkillsetCommand::Deactivate(_) => "skillset.deactivate",
            SkillsetCommand::Eval(_) => "skillset.eval",
            SkillsetCommand::Release(_) => "skillset.release",
            SkillsetCommand::Rollback(_) => "skillset.rollback",
        },
        Command::Telemetry { command } => match command {
            TelemetryCommand::Status => "telemetry.status",
            TelemetryCommand::Enable(_) => "telemetry.enable",
            TelemetryCommand::Disable => "telemetry.disable",
            TelemetryCommand::Report(_) => "telemetry.report",
            TelemetryCommand::Export(_) => "telemetry.export",
            TelemetryCommand::Purge(_) => "telemetry.purge",
        },
        Command::Provider { command } => match command {
            crate::cli::ProviderCommand::Add(_) => "provider.add",
            crate::cli::ProviderCommand::List => "provider.list",
            crate::cli::ProviderCommand::Remove(_) => "provider.remove",
        },
        Command::Catalog { command } => match command {
            crate::cli::CatalogCommand::Search(_) => "catalog.search",
            crate::cli::CatalogCommand::Show(_) => "catalog.show",
            crate::cli::CatalogCommand::Preview(_) => "catalog.preview",
        },
        Command::Package { command } => match command {
            PackageCommand::Plan(_) => "package.plan",
            PackageCommand::Build(_) => "package.build",
            PackageCommand::Verify(_) => "package.verify",
        },
        Command::Mcp { command } => match command {
            McpCommand::Requirement { command } => match command {
                McpRequirementCommand::List(_) => "mcp.requirement.list",
            },
            McpCommand::Plan(_) => "mcp.plan",
            McpCommand::Doctor(_) => "mcp.doctor",
            McpCommand::Catalog { command } => match command {
                McpCatalogCommand::Search(_) => "mcp.catalog.search",
                McpCatalogCommand::Show(_) => "mcp.catalog.show",
            },
        },
        Command::Provision { command } => match command {
            ProvisionCommand::Plan(_) => "provision.plan",
            ProvisionCommand::Apply(_) => "provision.apply",
            ProvisionCommand::Doctor(_) => "provision.doctor",
            ProvisionCommand::Export(_) => "provision.export",
            ProvisionCommand::Import(_) => "provision.import",
        },
        Command::Policy { command } => match command {
            PolicyCommand::Org { command } => match command {
                OrgPolicyCommand::Init(_) => "policy.org.init",
                OrgPolicyCommand::Show => "policy.org.show",
                OrgPolicyCommand::Check(_) => "policy.org.check",
            },
        },
        Command::Approval { command } => match command {
            ApprovalCommand::Request(_) => "approval.request",
            ApprovalCommand::List(_) => "approval.list",
            ApprovalCommand::Approve(_) => "approval.approve",
            ApprovalCommand::Reject(_) => "approval.reject",
        },
        Command::Roles { command } => match command {
            RolesCommand::List => "roles.list",
            RolesCommand::Grant(_) => "roles.grant",
            RolesCommand::Revoke(_) => "roles.revoke",
        },
        Command::Instruction { command } => match command {
            InstructionCommand::Scan(_) => "instruction.scan",
            InstructionCommand::Show(_) => "instruction.show",
            InstructionCommand::Classify(_) => "instruction.classify",
            InstructionCommand::Doctor(_) => "instruction.doctor",
            InstructionCommand::MigratePlan(_) => "instruction.migrate_plan",
        },
        Command::Workflow { command } => match command {
            WorkflowCommand::Create(_) => "workflow.create",
            WorkflowCommand::Show(_) => "workflow.show",
            WorkflowCommand::Plan(_) => "workflow.plan",
            WorkflowCommand::Preflight(_) => "workflow.preflight",
            WorkflowCommand::Run(_) => "workflow.run",
        },
        Command::Index(args) if args.action == "build" => "index.build",
        Command::Index(args) if args.action == "status" => "index.status",
        Command::Index(_) => "index",
        Command::Active(args) if args.action == "recommend" => "active.recommend",
        Command::Active(_) => "active",
        Command::Sync { command } => match command {
            SyncCommand::Status => "sync.status",
            SyncCommand::Push(_) => "sync.push",
            SyncCommand::Pull => "sync.pull",
            SyncCommand::Replay => "sync.replay",
        },
        Command::Ops { command } => match command {
            OpsCommand::List => "ops.list",
            OpsCommand::Retry => "ops.retry",
            OpsCommand::Purge => "ops.purge",
            OpsCommand::History { command } => match command {
                OpsHistoryCommand::Diagnose => "ops.history.diagnose",
                OpsHistoryCommand::Repair(_) => "ops.history.repair",
            },
        },
        Command::Agent { command } => match command {
            AgentCommand::Preflight(_) => "agent.preflight",
        },
        Command::Codex { command } => match command {
            CodexCommand::Reconcile(_) => "codex.reconcile",
        },
        Command::Panel(_) => "panel",
    }
}

// ---------------------------------------------------------------------------
// Enum-to-str converters
// ---------------------------------------------------------------------------

pub(crate) fn agent_kind_as_str(agent: AgentKind) -> &'static str {
    agent.as_str()
}

pub(crate) fn workspace_matcher_kind_as_str(kind: WorkspaceMatcherKind) -> &'static str {
    kind.as_str()
}

pub(crate) fn target_ownership_as_str(ownership: crate::cli::TargetOwnership) -> &'static str {
    ownership.as_str()
}

pub(crate) fn target_capabilities(
    ownership: crate::cli::TargetOwnership,
) -> RegistryTargetCapabilities {
    match ownership {
        crate::cli::TargetOwnership::Managed => RegistryTargetCapabilities {
            symlink: true,
            copy: true,
            watch: true,
        },
        crate::cli::TargetOwnership::Observed => RegistryTargetCapabilities {
            symlink: false,
            copy: false,
            watch: true,
        },
        crate::cli::TargetOwnership::External => RegistryTargetCapabilities {
            symlink: false,
            copy: false,
            watch: false,
        },
    }
}

pub(crate) fn projection_method_as_str(method: ProjectionMethod) -> &'static str {
    method.as_str()
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

pub(crate) fn validate_non_empty(
    name: &str,
    value: &str,
) -> std::result::Result<(), CommandFailure> {
    if value.trim().is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("--{} must not be empty", name),
        ));
    }
    Ok(())
}

pub(crate) fn validate_policy_profile(value: &str) -> std::result::Result<(), CommandFailure> {
    if !(1..=64).contains(&value.len()) {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--policy-profile must be 1-64 characters",
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--policy-profile must match [a-z0-9_-]{1,64}",
        ));
    }
    Ok(())
}

pub(crate) fn validate_projection_method(
    target: &RegistryProjectionTarget,
    method: ProjectionMethod,
) -> std::result::Result<(), CommandFailure> {
    match method {
        ProjectionMethod::Symlink if !target.capabilities.symlink => Err(CommandFailure::new(
            ErrorCode::ProjectionMethodUnsupported,
            format!(
                "target '{}' does not support symlink projections",
                target.target_id
            ),
        )),
        ProjectionMethod::Copy | ProjectionMethod::Materialize if !target.capabilities.copy => {
            Err(CommandFailure::new(
                ErrorCode::ProjectionMethodUnsupported,
                format!(
                    "target '{}' does not support copy/materialize projections",
                    target.target_id
                ),
            ))
        }
        _ => Ok(()),
    }
}

pub(crate) fn commit_registry_state(
    ctx: &AppContext,
    message: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    crate::gitops::commit_paths_if_changed(
        ctx,
        &[".gitignore", ".gitattributes", "state/registry", "state/v3"],
        message,
    )
    .map_err(map_git)
}

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

pub(crate) fn unique_target_id_for_agent(
    agent: &str,
    path: &str,
    targets: &RegistryTargetsFile,
) -> String {
    let token = target_path_token(path, agent);
    let base = format!("target_{}_{}", slugify(agent), slugify(&token));
    unique_id(
        &base,
        targets
            .targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect(),
    )
}

pub(crate) fn target_path_token(path: &str, agent: &str) -> String {
    let route = Path::new(path);
    let leaf = route
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(agent);

    // Names like ".../skills" are too generic. Include the parent to keep ids readable:
    // ".claude/skills" => "claude_skills", ".claude-work/skills" => "claude-work_skills".
    if (leaf.eq_ignore_ascii_case("skills") || leaf.eq_ignore_ascii_case("skill"))
        && let Some(parent) = route
            .parent()
            .and_then(|value| value.file_name())
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
    {
        return format!("{}_{}", parent, leaf);
    }

    leaf.to_string()
}

pub(crate) fn unique_binding_id(
    bindings: &crate::state_model::RegistryBindingsFile,
    args: &BindingAddArgs,
) -> String {
    let matcher_token = binding_matcher_token(args);
    let base = format!(
        "bind_{}_{}",
        agent_kind_as_str(args.agent),
        slugify(&matcher_token)
    );
    unique_id(
        &base,
        bindings
            .bindings
            .iter()
            .map(|binding| binding.binding_id.as_str())
            .collect(),
    )
}

fn binding_matcher_token(args: &BindingAddArgs) -> String {
    match args.matcher_kind {
        WorkspaceMatcherKind::PathPrefix | WorkspaceMatcherKind::ExactPath => {
            Path::new(&args.matcher_value)
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or(&args.profile)
                .to_string()
        }
        WorkspaceMatcherKind::Name => args.matcher_value.clone(),
    }
}

pub(crate) fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }

    let normalized = out.trim_matches('_');
    if normalized.is_empty() {
        "item".to_string()
    } else {
        normalized.to_string()
    }
}

fn unique_id(base: &str, existing: Vec<&str>) -> String {
    let existing = existing
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if !existing.contains(base) {
        return base.to_string();
    }

    for index in 2..1000 {
        let candidate = format!("{}_{}", base, index);
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }

    format!("{}_{}", base, Uuid::new_v4().simple())
}

pub(crate) fn projection_instance_id(skill: &str, binding_id: &str, target_id: &str) -> String {
    format!(
        "inst_{}_{}_{}",
        slugify(skill),
        slugify(binding_id),
        slugify(target_id)
    )
}

pub(crate) fn shell_arg(value: impl AsRef<Path>) -> String {
    let raw = value.as_ref().display().to_string();
    if raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        raw
    } else {
        format!("'{}'", raw.replace('\'', "'\\''"))
    }
}

// ---------------------------------------------------------------------------
// Error mappers
// ---------------------------------------------------------------------------

pub(crate) fn map_project_io(
    method: ProjectionMethod,
) -> impl FnOnce(anyhow::Error) -> CommandFailure {
    move |err| {
        CommandFailure::new(
            ErrorCode::IoError,
            format!(
                "failed to project skill using {}: {}",
                projection_method_as_str(method),
                err
            ),
        )
    }
}

pub(crate) fn map_arg(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, err.to_string())
}

pub(crate) fn map_io<E: std::fmt::Display>(err: E) -> CommandFailure {
    CommandFailure::new(ErrorCode::IoError, err.to_string())
}

pub(crate) fn map_git(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::GitError, err.to_string())
}

pub(crate) fn map_lock(err: anyhow::Error) -> CommandFailure {
    let message = err.to_string();
    if let Some(rest) = message.strip_prefix("ARG_INVALID:") {
        return CommandFailure::new(ErrorCode::ArgInvalid, rest.trim());
    }
    CommandFailure::new(ErrorCode::LockBusy, message)
}

pub(crate) fn map_remote_unreachable(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::RemoteUnreachable, err.to_string())
}

pub(crate) fn map_push_rejected(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::PushRejected, err.to_string())
}

pub(crate) fn map_replay_conflict(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::ReplayConflict, err.to_string())
}

pub(crate) fn map_registry_state(err: anyhow::Error) -> CommandFailure {
    let message = err.to_string();
    if message.contains("schema version mismatch") {
        CommandFailure::new(ErrorCode::SchemaMismatch, message)
    } else {
        CommandFailure::new(ErrorCode::StateCorrupt, message)
    }
}
