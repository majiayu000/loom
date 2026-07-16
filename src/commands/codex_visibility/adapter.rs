use serde_json::{Value, json};

use crate::agent_adapters::AgentAdapter;

use super::{CodexVisibilityReport, check};

pub(super) fn adapter_has_visibility_metadata(adapter: &AgentAdapter) -> bool {
    if adapter.source == "built-in" {
        return adapter.fidelity.is_verified();
    }
    adapter.adapter_api == "2" && !adapter.visibility.identity_by_projection_method.is_empty()
}

pub(super) fn unsupported_visibility_message(adapter: &AgentAdapter) -> String {
    if adapter.fidelity.is_verified() {
        format!(
            "agent adapter '{}' does not expose visibility metadata",
            adapter.id
        )
    } else {
        format!(
            "agent adapter '{}' has generic fidelity and does not expose verified visibility metadata",
            adapter.id
        )
    }
}

pub(super) fn unsupported_visibility_report(
    skill: &str,
    agent: &str,
    fidelity: Option<&str>,
    message: String,
    details: Value,
) -> CodexVisibilityReport {
    CodexVisibilityReport {
        skill: skill.to_string(),
        agent: agent.to_string(),
        fidelity: fidelity.map(str::to_string),
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
        "fidelity": adapter.fidelity.as_str(),
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
