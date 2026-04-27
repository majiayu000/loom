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

use crate::cli::{Cli, Command, SkillCommand, WorkspaceCommand};
use crate::envelope::{Envelope, Meta};
use crate::state::AppContext;
use crate::types::ErrorCode;

pub use helpers::{collect_skill_inventory, remote_status_payload};

use helpers::{command_name, ensure_initial_commit, map_git, map_io};

use crate::gitops;
use crate::state_model::V3StatePaths;

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
        let request_id = cli.request_id.unwrap_or_else(|| Uuid::new_v4().to_string());

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
            },
            Command::Sync { command } => self.cmd_sync(command),
            Command::Ops { command } => self.cmd_ops(command),
            Command::Panel(_) => Ok((json!({"message": "panel handled in main"}), Meta::default())),
        };

        match result {
            Ok((data, meta)) => {
                let env = Envelope::ok(command_name(&cli.command), request_id, data, meta);
                Ok((env, 0))
            }
            Err(f) => {
                let env = Envelope::err(
                    command_name(&cli.command),
                    request_id,
                    f.code,
                    f.message,
                    f.details,
                );
                Ok((env, f.code.exit_code()))
            }
        }
    }

    pub(crate) fn require_v3_snapshot(
        &self,
    ) -> std::result::Result<crate::state_model::V3Snapshot, CommandFailure> {
        let paths = V3StatePaths::from_app_context(&self.ctx);
        match paths.maybe_load_snapshot().map_err(helpers::map_v3_state)? {
            Some(snapshot) => Ok(snapshot),
            None => Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("v3 state not initialized under {}", paths.v3_dir.display()),
            )),
        }
    }

    pub(crate) fn ensure_v3_layout(&self) -> std::result::Result<V3StatePaths, CommandFailure> {
        let paths = V3StatePaths::from_app_context(&self.ctx);
        paths.ensure_layout().map_err(helpers::map_v3_state)?;
        Ok(paths)
    }
}
