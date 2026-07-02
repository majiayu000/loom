use serde_json::{Value, json};

use crate::cli::AgentKind;

use super::super::skill_deps::SkillDependencyReport;

pub(super) fn dependency_agent(agent: Option<AgentKind>) -> Option<&'static str> {
    match agent {
        Some(AgentKind::Codex) => Some("codex"),
        _ => None,
    }
}

pub(super) fn add_dependency_checks(
    dependencies: Option<&SkillDependencyReport>,
    checks: &mut Vec<Value>,
) {
    let Some(dependencies) = dependencies else {
        return;
    };
    let readiness_severity = if dependencies.status == "unknown" {
        "warning"
    } else {
        "error"
    };
    let mut next_actions = dependencies.next_actions.clone();
    if let Some(action) = mcp_plan_next_action(dependencies)
        && !next_actions.iter().any(|existing| existing == &action)
    {
        next_actions.push(action);
    }
    checks.push(super::check(
        "dependencies",
        "dependency_readiness",
        dependencies.ready,
        readiness_severity,
        if dependencies.ready {
            "runtime dependencies are ready"
        } else {
            "runtime dependency readiness failed"
        },
        "run loom skill deps and resolve dependency next actions",
        json!({
            "status": dependencies.status,
            "next_actions": next_actions
        }),
    ));
    for finding in &dependencies.findings {
        checks.push(super::check(
            "dependencies",
            &format!("skill_dependency:{}", finding.id),
            false,
            &finding.severity,
            &finding.message,
            &finding.suggested_action,
            finding.details.clone(),
        ));
    }
}

fn mcp_plan_next_action(dependencies: &SkillDependencyReport) -> Option<String> {
    dependencies.findings.iter().find_map(|finding| {
        if finding.id != "mcp_missing" && finding.id != "mcp_status_unknown" {
            return None;
        }
        let agent = finding.details.get("agent")?.as_str()?;
        Some(format!(
            "loom mcp plan --skill {} --agent {}",
            dependencies.skill, agent
        ))
    })
}
