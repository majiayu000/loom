use std::path::Path;

use anyhow::{Result, anyhow};
use uuid::Uuid;

use crate::cli::{
    AgentKind, BindingAddArgs, Command, OpsCommand, OpsHistoryCommand, ProjectionMethod,
    SkillCommand, SyncCommand, TargetAddArgs, TargetCommand, WorkspaceBindingCommand,
    WorkspaceCommand, WorkspaceMatcherKind,
};
use crate::state::AppContext;
use crate::state_model::{V3ProjectionTarget, V3TargetCapabilities, V3TargetsFile};
use crate::types::ErrorCode;

use super::CommandFailure;

// Re-export items from sibling modules so existing `use super::helpers::*` paths keep working.
pub(crate) use super::file_ops::{
    backup_path_if_exists, copy_dir_recursive, copy_dir_recursive_without_symlinks, read_git_field,
    rollback_added_skill,
};
pub use super::projections::{collect_skill_inventory, remote_status_payload};
pub(crate) use super::projections::{
    maybe_autosync_or_queue, project_skill_to_target, record_v3_operation,
    remote_status_payload_with_pending, resolve_capture_projection, sync_push_internal,
    sync_replay_internal, update_projection_after_capture, upsert_projection, upsert_rule,
};

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
            SkillCommand::Add(_) => "skill.add",
            SkillCommand::Project(_) => "skill.project",
            SkillCommand::Capture(_) => "skill.capture",
            SkillCommand::Save(_) => "skill.save",
            SkillCommand::Snapshot(_) => "skill.snapshot",
            SkillCommand::Release(_) => "skill.release",
            SkillCommand::Rollback(_) => "skill.rollback",
            SkillCommand::Diff(_) => "skill.diff",
            SkillCommand::ImportObserved(_) => "skill.import-observed",
        },
        Command::Sync { command } => match command {
            SyncCommand::Status => "sync.status",
            SyncCommand::Push => "sync.push",
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
        Command::Panel(_) => "panel",
    }
}

// ---------------------------------------------------------------------------
// Enum-to-str converters
// ---------------------------------------------------------------------------

pub(crate) fn agent_kind_as_str(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Cursor => "cursor",
        AgentKind::Windsurf => "windsurf",
        AgentKind::Cline => "cline",
        AgentKind::Copilot => "copilot",
        AgentKind::Aider => "aider",
        AgentKind::Opencode => "opencode",
        AgentKind::GeminiCli => "gemini-cli",
        AgentKind::Goose => "goose",
    }
}

pub(crate) fn workspace_matcher_kind_as_str(kind: WorkspaceMatcherKind) -> &'static str {
    match kind {
        WorkspaceMatcherKind::PathPrefix => "path_prefix",
        WorkspaceMatcherKind::ExactPath => "exact_path",
        WorkspaceMatcherKind::Name => "name",
    }
}

pub(crate) fn target_ownership_as_str(ownership: crate::cli::TargetOwnership) -> &'static str {
    match ownership {
        crate::cli::TargetOwnership::Managed => "managed",
        crate::cli::TargetOwnership::Observed => "observed",
        crate::cli::TargetOwnership::External => "external",
    }
}

pub(crate) fn target_capabilities(ownership: crate::cli::TargetOwnership) -> V3TargetCapabilities {
    match ownership {
        crate::cli::TargetOwnership::Managed => V3TargetCapabilities {
            symlink: true,
            copy: true,
            watch: true,
        },
        crate::cli::TargetOwnership::Observed => V3TargetCapabilities {
            symlink: false,
            copy: false,
            watch: true,
        },
        crate::cli::TargetOwnership::External => V3TargetCapabilities {
            symlink: false,
            copy: false,
            watch: false,
        },
    }
}

pub(crate) fn projection_method_as_str(method: ProjectionMethod) -> &'static str {
    match method {
        ProjectionMethod::Symlink => "symlink",
        ProjectionMethod::Copy => "copy",
        ProjectionMethod::Materialize => "materialize",
    }
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

pub(crate) fn validate_projection_method(
    target: &V3ProjectionTarget,
    method: ProjectionMethod,
) -> std::result::Result<(), CommandFailure> {
    match method {
        ProjectionMethod::Symlink if !target.capabilities.symlink => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "target '{}' does not support symlink projections",
                target.target_id
            ),
        )),
        ProjectionMethod::Copy | ProjectionMethod::Materialize if !target.capabilities.copy => {
            Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' does not support copy/materialize projections",
                    target.target_id
                ),
            ))
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// ID generation
// ---------------------------------------------------------------------------

pub(crate) fn unique_target_id(targets: &V3TargetsFile, args: &TargetAddArgs) -> String {
    unique_target_id_for(args.agent, &args.path, targets)
}

fn unique_target_id_for(agent: AgentKind, path: &str, targets: &V3TargetsFile) -> String {
    let token = target_path_token(path, agent);
    let base = format!("target_{}_{}", agent_kind_as_str(agent), slugify(&token));
    unique_id(
        &base,
        targets
            .targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect(),
    )
}

pub(crate) fn target_path_token(path: &str, agent: AgentKind) -> String {
    let route = Path::new(path);
    let leaf = route
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(agent_kind_as_str(agent));

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
    bindings: &crate::state_model::V3BindingsFile,
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

pub(crate) fn map_queue<E: std::fmt::Display>(err: E) -> CommandFailure {
    CommandFailure::new(ErrorCode::QueueBlocked, err.to_string())
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

pub(crate) fn map_v3_state(err: anyhow::Error) -> CommandFailure {
    let message = err.to_string();
    if message.contains("schema version mismatch") {
        CommandFailure::new(ErrorCode::SchemaMismatch, message)
    } else {
        CommandFailure::new(ErrorCode::StateCorrupt, message)
    }
}
