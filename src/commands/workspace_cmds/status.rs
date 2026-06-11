use serde_json::json;

use crate::envelope::Meta;
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::super::file_ops::read_git_field;
use super::super::helpers::{map_io, map_registry_state};
use super::super::projections::{collect_skill_inventory, remote_status_payload_with_pending};
use super::super::{App, CommandFailure};

impl App {
    pub fn cmd_status(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_inventory = collect_skill_inventory(&self.ctx);
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let pending_ops = pending_report.ops.len();
        let target_dirs = resolve_agent_skill_dirs(&self.ctx.root);
        let registry_paths = RegistryStatePaths::from_app_context(&self.ctx);
        let legacy_state_dir_present = registry_paths.legacy_state_dir_exists();
        let registry_status = registry_paths
            .maybe_load_snapshot()
            .map_err(map_registry_state)?
            .map(|snapshot| snapshot.status_view())
            .unwrap_or_else(|| {
                json!({
                    "state_model": "registry",
                    "available": false,
                    "error": {
                        "code": if legacy_state_dir_present {
                            ErrorCode::SchemaMismatch.as_str()
                        } else {
                            ErrorCode::StateNotInitialized.as_str()
                        },
                        "message": if legacy_state_dir_present {
                            format!(
                                "legacy registry state found under {}; run a write command to migrate it",
                                registry_paths.state_dir.join("v3").display()
                            )
                        } else {
                            format!("registry state not initialized under {}", registry_paths.registry_dir.display())
                        }
                    }
                })
            });
        let (registered_target_count, registered_target_ids) = registry_status
            .get("targets")
            .and_then(|value| value.as_array())
            .map(|targets| {
                let ids = targets
                    .iter()
                    .filter_map(|target| target.get("target_id").and_then(|id| id.as_str()))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>();
                (targets.len(), ids)
            })
            .unwrap_or((0, Vec::new()));
        let mut git_warnings = Vec::new();
        let head = read_git_field(&self.ctx, &["rev-parse", "HEAD"], &mut git_warnings);
        let branch = read_git_field(
            &self.ctx,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            &mut git_warnings,
        );
        let status_short = read_git_field(&self.ctx, &["status", "--short"], &mut git_warnings);

        let (remote, mut meta) = remote_status_payload_with_pending(&self.ctx, pending_report)?;
        meta.warnings.splice(0..0, git_warnings);
        meta.warnings.extend(skill_inventory.warnings);
        let source_skill_sample = skill_inventory
            .source_skills
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>();

        let data = json!({
            "state_model": "registry",
            "inventory": {
                "source_skill_count": skill_inventory.source_skills.len(),
                "source_skill_sample": source_skill_sample,
                "source_skill_sample_truncated": skill_inventory.source_skills.len() > 20,
                "backup_skill_count": skill_inventory.backup_skills.len(),
                "source_dirs": skill_inventory
                        .source_dirs
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>(),
            },
            "backup_dir": self.ctx.skills_dir.display().to_string(),
            "git": {"head": head, "branch": branch, "status_short": status_short},
            "agent_dir_defaults": {
                "claude_dir": target_dirs.claude.display().to_string(),
                "codex_dir": target_dirs.codex.display().to_string(),
                "agent_dirs": target_dirs
                    .all
                    .iter()
                    .map(|dir| json!({
                        "agent": dir.agent,
                        "env_var": dir.env_var,
                        "path": dir.path.display().to_string()
                    }))
                    .collect::<Vec<_>>()
            },
            "registered_targets": {
                "count": registered_target_count,
                "target_ids": registered_target_ids
            },
            "remote": remote,
            "pending_ops": pending_ops,
            "registry": registry_status
        });

        Ok((data, meta))
    }
}
