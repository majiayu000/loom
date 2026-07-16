use std::path::Path;

use serde_json::{Value, json};

use crate::agent_adapters::AgentAdapter;
use crate::state_model::{RegistryBindingRule, RegistrySnapshot, RegistryWorkspaceBinding};

use super::{CODEX_AGENT, CodexVisibilityCheck, CodexVisibilityReport, normalize_existing_or_raw};

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

pub(super) fn adapter_has_visibility_metadata(adapter: &AgentAdapter) -> bool {
    if adapter.source == "built-in" {
        return matches!(adapter.id.as_str(), CODEX_AGENT | "claude");
    }
    adapter.adapter_api == "2" && !adapter.visibility.identity_by_projection_method.is_empty()
}

pub(super) fn unsupported_visibility_report(
    skill: &str,
    agent: &str,
    message: String,
    details: Value,
) -> CodexVisibilityReport {
    CodexVisibilityReport {
        skill: skill.to_string(),
        agent: agent.to_string(),
        visible: false,
        checks: vec![check(
            "visibility_unsupported",
            false,
            "error",
            &message,
            details,
            Some(format!(
                "install or update the {agent} adapter visibility metadata"
            )),
        )],
        next_actions: vec![format!(
            "install or update the {agent} adapter visibility metadata"
        )],
        restart_required: false,
    }
}

pub(super) fn adapter_metadata_details(adapter: &AgentAdapter) -> Value {
    json!({
        "adapter_id": adapter.id,
        "adapter_source": adapter.source,
        "skill_entrypoint": adapter.skill_entrypoint,
        "projection_methods": adapter.projection_methods,
        "discovery_roots": adapter.discovery_roots.iter().map(|root| {
            json!({
                "scope": root.scope,
                "path_template": root.path_template,
                "role": root.role,
                "source_env_var": root.source_env_var,
                "priority": root.priority,
                "scan_eligible": root.scan_eligible,
                "available": root.available,
                "unavailable_reason": root.unavailable_reason,
            })
        }).collect::<Vec<_>>(),
        "visibility": adapter_visibility_details(adapter),
        "reload": {
            "strategy": adapter.reload.strategy,
            "hot_reload": adapter.reload.hot_reload,
            "notes": adapter.reload.notes,
        },
    })
}

pub(super) fn adapter_visibility_details(adapter: &AgentAdapter) -> Value {
    json!({
        "follows_symlink_dirs": adapter.visibility.follows_symlink_dirs,
        "identity_by_projection_method": adapter.visibility.identity_by_projection_method,
        "config_file": adapter.visibility.config_file,
        "disable_rules": adapter.visibility.disable_rules,
    })
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
