mod event_store;
mod file_ops;
mod fs_probe;
mod helpers;
mod projections;
mod skill_cmds;
mod sync_cmds;
mod target_cmds;
mod version_cmds;
mod workspace_cmds;

use anyhow::Result;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{
    Cli, Command, OpsCommand, OpsHistoryCommand, RemoteCommand, SkillCommand, SkillOrphanCommand,
    SyncCommand, TargetCommand, WorkspaceBindingCommand, WorkspaceCommand,
};
use crate::envelope::{Envelope, Meta};
use crate::state::AppContext;
use crate::types::ErrorCode;

pub use helpers::{collect_skill_inventory, remote_status_payload};

use event_store::{
    append_command_audit_failure, append_command_finished, append_command_started,
    command_event_input, prepare_command_event_store,
};
use helpers::{command_name, ensure_initial_commit, map_git, map_io};

use crate::gitops;
use crate::state_model::RegistryStatePaths;

#[derive(Debug)]
pub struct CommandFailure {
    pub code: ErrorCode,
    pub message: String,
    pub details: serde_json::Value,
}

impl CommandFailure {
    pub(crate) fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: json!({}),
        }
    }
}

pub struct App {
    pub ctx: AppContext,
}

impl App {
    pub fn new(root: Option<std::path::PathBuf>) -> Result<Self> {
        let ctx = AppContext::new(root)?;
        Ok(Self { ctx })
    }

    pub(crate) fn ensure_write_layout(&self) -> std::result::Result<(), CommandFailure> {
        self.ctx.ensure_state_layout().map_err(map_io)?;
        Ok(())
    }

    pub(crate) fn ensure_write_repo_ready(&self) -> std::result::Result<(), CommandFailure> {
        self.ensure_write_layout()?;
        gitops::ensure_repo_initialized(&self.ctx).map_err(map_git)?;
        self.ctx.ensure_gitignore_entries().map_err(map_io)?;
        ensure_initial_commit(&self.ctx).map_err(map_git)?;
        Ok(())
    }

    pub fn execute(&self, cli: Cli) -> Result<(Envelope, i32)> {
        let cmd = command_name(&cli.command);
        let request_id = cli
            .request_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let input = command_event_input(&cli, &request_id);
        let audit_required = command_requires_durable_audit(&cli.command);
        let mut audit_started = false;
        let mut audit_warning = None;
        match prepare_command_event_store(&self.ctx) {
            Ok(()) => match append_command_started(&self.ctx, cmd, input, &request_id) {
                Ok(()) => audit_started = true,
                Err(err) if audit_required => {
                    let env = Envelope::err(
                        cmd,
                        request_id,
                        ErrorCode::InternalError,
                        format!("failed to append command event: {}", err),
                        json!({}),
                    );
                    return Ok((env, ErrorCode::InternalError.exit_code()));
                }
                Err(err) => {
                    audit_warning = Some(format!("failed to append command event: {}", err));
                }
            },
            Err(err) if audit_required => {
                let env = Envelope::err(
                    cmd,
                    request_id,
                    ErrorCode::InternalError,
                    format!("failed to prepare command event log: {}", err),
                    json!({}),
                );
                return Ok((env, ErrorCode::InternalError.exit_code()));
            }
            Err(err) => {
                audit_warning = Some(format!("failed to prepare command event log: {}", err));
            }
        }

        let result = match &cli.command {
            Command::Workspace { command } => match command {
                WorkspaceCommand::Status => self.cmd_status(),
                WorkspaceCommand::Doctor => self.cmd_doctor(),
                WorkspaceCommand::Init(args) => self.cmd_workspace_init(args, &request_id),
                WorkspaceCommand::Binding { command } => {
                    self.cmd_workspace_binding(command, &request_id)
                }
                WorkspaceCommand::Remote { command } => self.cmd_remote(command),
            },
            Command::Target { command } => self.cmd_target(command, &request_id),
            Command::Skill { command } => match command {
                SkillCommand::Add(args) => self.cmd_add(args, &request_id),
                SkillCommand::ImportObserved(args) => self.cmd_import_observed(args, &request_id),
                SkillCommand::Project(args) => self.cmd_project(args, &request_id),
                SkillCommand::Capture(args) => self.cmd_capture(args, &request_id),
                SkillCommand::Save(args) => self.cmd_save(args, &request_id),
                SkillCommand::Snapshot(args) => self.cmd_snapshot(args, &request_id),
                SkillCommand::Release(args) => self.cmd_release(args, &request_id),
                SkillCommand::Rollback(args) => self.cmd_rollback(args, &request_id),
                SkillCommand::Diff(args) => self.cmd_diff(args),
                SkillCommand::Orphan {
                    command: SkillOrphanCommand::Clean(args),
                } => self.cmd_skill_orphan_clean(args, &request_id),
            },
            Command::Sync { command } => self.cmd_sync(command),
            Command::Ops { command } => self.cmd_ops(command),
            Command::Panel(_) => Ok((json!({"message": "panel handled in main"}), Meta::default())),
        };

        match result {
            Ok((data, meta)) => {
                let mut env = Envelope::ok(cmd, request_id, data, meta);
                if let Some(warning) = audit_warning.take() {
                    env.meta.warnings.push(warning);
                }
                Ok(self.finish_command_audit(cmd, env, 0, audit_started, audit_required))
            }
            Err(f) => {
                let exit_code = f.code.exit_code();
                let mut env = Envelope::err(cmd, request_id, f.code, f.message, f.details);
                if let Some(warning) = audit_warning.take() {
                    env.meta.warnings.push(warning);
                }
                Ok(self.finish_command_audit(cmd, env, exit_code, audit_started, audit_required))
            }
        }
    }

    fn finish_command_audit(
        &self,
        cmd: &str,
        mut env: Envelope,
        exit_code: i32,
        audit_started: bool,
        audit_required: bool,
    ) -> (Envelope, i32) {
        if !audit_started {
            return (env, exit_code);
        }

        if let Err(err) = append_command_finished(&self.ctx, cmd, &env, exit_code) {
            let warning = format!("failed to append command event: {}", err);
            if !audit_required {
                env.meta.warnings.push(warning);
                return (env, exit_code);
            }

            let failure_exit = ErrorCode::InternalError.exit_code();
            let mut failure_env = Envelope::err(
                cmd,
                env.request_id.clone(),
                ErrorCode::InternalError,
                warning,
                json!({
                    "audit_stage": "finish",
                    "original_ok": env.ok,
                    "original_exit_code": exit_code,
                    "original_error": env.error.as_ref().map(|error| {
                        json!({
                            "code": error.code,
                            "message": error.message,
                        })
                    }),
                }),
            );
            failure_env.meta.warnings = env.meta.warnings;
            if let Err(recovery_err) =
                append_command_audit_failure(&self.ctx, cmd, &failure_env, failure_exit)
            {
                failure_env.meta.warnings.push(format!(
                    "failed to append audit failure event after terminal append failure: {}",
                    recovery_err
                ));
            }
            return (failure_env, failure_exit);
        }

        (env, exit_code)
    }

    pub(crate) fn require_registry_snapshot(
        &self,
    ) -> std::result::Result<crate::state_model::RegistrySnapshot, CommandFailure> {
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        match paths
            .maybe_load_snapshot()
            .map_err(helpers::map_registry_state)?
        {
            Some(snapshot) => Ok(snapshot),
            None => Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "registry state not initialized under {}",
                    paths.registry_dir.display()
                ),
            )),
        }
    }

    pub(crate) fn ensure_registry_layout(
        &self,
    ) -> std::result::Result<RegistryStatePaths, CommandFailure> {
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        paths.ensure_layout().map_err(helpers::map_registry_state)?;
        Ok(paths)
    }
}

fn command_requires_durable_audit(command: &Command) -> bool {
    match command {
        Command::Workspace { command } => match command {
            WorkspaceCommand::Status | WorkspaceCommand::Doctor => false,
            WorkspaceCommand::Init(_) => true,
            WorkspaceCommand::Binding { command } => !matches!(
                command,
                WorkspaceBindingCommand::List | WorkspaceBindingCommand::Show(_)
            ),
            WorkspaceCommand::Remote { command } => !matches!(command, RemoteCommand::Status),
        },
        Command::Target { command } => {
            !matches!(command, TargetCommand::List | TargetCommand::Show(_))
        }
        Command::Skill { command } => matches!(
            command,
            SkillCommand::Add(_)
                | SkillCommand::ImportObserved(_)
                | SkillCommand::Project(_)
                | SkillCommand::Capture(_)
                | SkillCommand::Save(_)
                | SkillCommand::Snapshot(_)
                | SkillCommand::Release(_)
                | SkillCommand::Rollback(_)
                | SkillCommand::Orphan {
                    command: SkillOrphanCommand::Clean(_)
                }
        ),
        Command::Sync { command } => !matches!(command, SyncCommand::Status),
        Command::Ops { command } => match command {
            OpsCommand::List => false,
            OpsCommand::Retry | OpsCommand::Purge => true,
            OpsCommand::History { command } => !matches!(command, OpsHistoryCommand::Diagnose),
        },
        Command::Panel(_) => false,
    }
}
