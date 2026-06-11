use serde_json::json;

use crate::cli::RemoteCommand;
use crate::envelope::Meta;
use crate::gitops;
use crate::types::ErrorCode;

use super::super::helpers::{map_git, map_lock};
use super::super::projections::remote_status_payload;
use super::super::{App, CommandFailure};

impl App {
    pub fn cmd_remote(
        &self,
        command: &RemoteCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            RemoteCommand::Set { url } => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                gitops::validate_git_url(url).map_err(|err| {
                    CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        format!("invalid remote url '{}': {}", url, err),
                    )
                })?;
                gitops::set_remote_origin(&self.ctx, url).map_err(map_git)?;
                Ok((json!({"remote": "origin", "url": url}), Meta::default()))
            }
            RemoteCommand::Status => {
                let (remote, meta) = remote_status_payload(&self.ctx)?;
                Ok((json!({"remote": remote}), meta))
            }
        }
    }
}
