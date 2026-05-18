use axum::{Json, extract::State};
use serde_json::json;

use crate::commands::{collect_skill_inventory, redact_sensitive_string};
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::RegistryStatePaths;

use super::super::auth::registry_ok_with_warnings;
use super::super::auth::registry_ok;
use super::super::PanelState;

pub(crate) async fn info(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let target_dirs = resolve_agent_skill_dirs(&state.ctx.root);
    let registry_paths = RegistryStatePaths::from_app_context(&state.ctx);

    let mut warnings: Vec<String> = Vec::new();
    let remote_url = match crate::gitops::remote_url(&state.ctx) {
        Ok(Some(url)) => redact_sensitive_string(&url),
        Ok(None) => {
            // `gitops::remote_url` returns `Ok(None)` for both "no remote
            // configured" (exit 2 "No such remote 'origin'") and "not a git
            // repository" (exit 128). Probe with `rev-parse --git-dir` to
            // distinguish the two so a missing or corrupt repository is
            // surfaced as a warning instead of being indistinguishable from
            // an unconfigured remote.
            match crate::gitops::run_git_allow_failure(&state.ctx, &["rev-parse", "--git-dir"]) {
                Ok(probe) if !probe.status.success() => {
                    warnings.push(format!(
                        "git repository not initialized at {}",
                        state.ctx.root.display()
                    ));
                }
                Err(err) => {
                    warnings.push(format!("failed to probe git repository: {err}"));
                }
                Ok(_) => {}
            }
            String::new()
        }
        Err(err) => {
            warnings.push(format!("failed to read git remote url: {err}"));
            String::new()
        }
    };

    registry_ok_with_warnings(
        "panel.info",
        json!({
            "root": state.ctx.root.display().to_string(),
            "state_dir": state.ctx.state_dir.display().to_string(),
            "registry_targets_file": registry_paths.targets_file.display().to_string(),
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
                .collect::<Vec<_>>(),
            "remote_url": remote_url,
        }),
        warnings,
    )
}

pub(crate) async fn skills(State(state): State<PanelState>) -> Json<serde_json::Value> {
    let inventory = collect_skill_inventory(&state.ctx);
    registry_ok(
        "panel.skills",
        json!({
            "skills": inventory.source_skills,
            "backup_skills": inventory.backup_skills,
            "source_dirs": inventory
                .source_dirs
                .iter()
                .map(|path: &std::path::PathBuf| path.display().to_string())
                .collect::<Vec<_>>(),
            "warnings": inventory.warnings
        }),
    )
}
