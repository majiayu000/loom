use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::commands::{CommandFailure, projection_path_is_safe_symlink};
use crate::error_actions::NextAction;
use crate::gemini_cli;
use crate::next_action_trace::observe_next_actions;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::{AgentAdapter, SOURCE_BUILT_IN};

pub(crate) fn built_in_projection_root(
    ctx: &AppContext,
    adapter: &AgentAdapter,
    scope: &str,
    workspace: &Path,
    skill: &str,
) -> std::result::Result<Option<PathBuf>, CommandFailure> {
    if adapter.id != "gemini-cli" || adapter.source != SOURCE_BUILT_IN {
        return Ok(None);
    }
    let base = match scope {
        "user" => {
            let workspace = std::env::current_dir().map_err(|error| {
                CommandFailure::new(
                    ErrorCode::IoError,
                    format!("failed to resolve current workspace: {error}"),
                )
            })?;
            gemini_cli::runtime_home(&workspace).map_err(gemini_config_failure)?
        }
        "project" => workspace.to_path_buf(),
        _ => return Ok(None),
    };
    let alias = base.join(".agents/skills").join(skill);
    if (alias.exists() || fs::symlink_metadata(&alias).is_ok())
        && !projection_path_is_safe_symlink(&alias, &ctx.skill_path(skill))
    {
        return Err(alias_shadow_failure(scope, skill, alias, &base));
    }
    Ok(Some(base.join(".gemini/skills")))
}

fn gemini_config_failure(message: &'static str) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::AdapterInvalid, message);
    failure.details = json!({
        "reason": "gemini_cli_config_invalid",
        "agent": "gemini-cli",
    });
    failure
}

fn alias_shadow_failure(scope: &str, skill: &str, alias: PathBuf, base: &Path) -> CommandFailure {
    let native = base.join(".gemini/skills").join(skill);
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        format!(
            "Gemini CLI alias '{}' shadows the Loom native projection '{}'",
            alias.display(),
            native.display()
        ),
    );
    failure.details = json!({
        "reason": "gemini_alias_shadows_native_projection",
        "agent": "gemini-cli",
        "scope": scope,
        "skill": skill,
        "shadowing_path": alias,
        "native_path": native,
    });
    failure.next_actions = observe_next_actions(
        "gemini.projection.alias_shadow",
        vec![NextAction {
            cmd: format!("loom skill visibility {skill} --agent gemini-cli"),
            reason: "inspect and remove or rename the higher-priority alias before retrying"
                .to_string(),
        }],
    );
    failure
}
