use std::path::Path;

use serde_json::Value;

use crate::state_model::{RegistryBindingRule, RegistrySnapshot, RegistryWorkspaceBinding};

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::{CODEX_AGENT, CodexVisibilityCheck};

pub(super) fn active_rules_for_skill(
    snapshot: &RegistrySnapshot,
    skill: &str,
    agent: &str,
    workspace: Option<&Path>,
    profile: Option<&str>,
) -> std::result::Result<Vec<RegistryBindingRule>, CommandFailure> {
    let mut rules = Vec::new();
    for rule in snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.skill_id == skill)
    {
        let Some(binding) = snapshot.binding(&rule.binding_id) else {
            continue;
        };
        let Some(target) = snapshot.target(&rule.target_id) else {
            continue;
        };
        if binding.agent == agent
            && binding.active
            && target.agent == agent
            && profile.is_none_or(|profile| binding.profile_id == profile)
            && match workspace {
                None => true,
                Some(workspace) => binding_matches_workspace(binding, workspace)?,
            }
        {
            rules.push(rule.clone());
        }
    }
    Ok(rules)
}

pub(super) fn reconcile_next_action(agent: &str) -> String {
    if agent == CODEX_AGENT {
        "loom codex reconcile --apply".to_string()
    } else {
        format!("loom agent reconcile --agent {agent} --dry-run")
    }
}

pub(super) fn reload_check_id(agent: &str) -> String {
    if agent == CODEX_AGENT {
        "codex_restart_required".to_string()
    } else {
        format!("{agent}_reload_required")
    }
}

pub(super) fn binding_matches_workspace(
    binding: &RegistryWorkspaceBinding,
    workspace: &Path,
) -> std::result::Result<bool, CommandFailure> {
    let matcher = &binding.workspace_matcher;
    match matcher.kind.as_str() {
        "path_prefix" | "exact_path" => matcher.matches_workspace(workspace).map_err(map_io),
        // Visibility treats every name matcher as user-scoped. Legacy registry
        // snapshots store the profile name (for example `default`) here rather
        // than the activation command's newer `user` marker.
        "name" => Ok(true),
        _ => Ok(false),
    }
}

pub(super) fn skill_is_referenced(snapshot: &RegistrySnapshot, skill: &str) -> bool {
    snapshot
        .rules
        .rules
        .iter()
        .any(|rule| rule.skill_id == skill)
        || snapshot
            .projections
            .projections
            .iter()
            .any(|projection| projection.skill_id == skill)
}

pub(super) fn check(
    id: &str,
    ok: bool,
    failure_severity: &str,
    message: &str,
    details: Value,
    next_action: Option<String>,
) -> CodexVisibilityCheck {
    CodexVisibilityCheck {
        id: id.to_string(),
        ok,
        severity: if ok { "info" } else { failure_severity }.to_string(),
        message: message.to_string(),
        details,
        next_action: if ok { None } else { next_action },
    }
}
