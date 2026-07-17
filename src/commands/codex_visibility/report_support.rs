use std::path::Path;

use serde_json::Value;

use crate::state_model::{RegistryBindingRule, RegistrySnapshot, RegistryWorkspaceBinding};

use super::{CODEX_AGENT, CodexVisibilityCheck, normalize_existing_or_raw};

pub(super) fn active_rules_for_skill(
    snapshot: &RegistrySnapshot,
    skill: &str,
    agent: &str,
    workspace: Option<&Path>,
    profile: Option<&str>,
) -> Vec<RegistryBindingRule> {
    snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.skill_id == skill)
        .filter_map(|rule| {
            let binding = snapshot.binding(&rule.binding_id)?;
            let target = snapshot.target(&rule.target_id)?;
            if binding.agent == agent
                && binding.active
                && target.agent == agent
                && profile.is_none_or(|profile| binding.profile_id == profile)
                && workspace.is_none_or(|workspace| binding_matches_workspace(binding, workspace))
            {
                Some(rule.clone())
            } else {
                None
            }
        })
        .collect()
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
) -> bool {
    let expected = normalize_existing_or_raw(workspace);
    let matcher = &binding.workspace_matcher;
    match matcher.kind.as_str() {
        "path_prefix" => expected.starts_with(normalize_existing_or_raw(Path::new(&matcher.value))),
        "exact_path" => expected == normalize_existing_or_raw(Path::new(&matcher.value)),
        "name" => true,
        _ => false,
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
