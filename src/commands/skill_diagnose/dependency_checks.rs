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
            "next_actions": dependencies.next_actions
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
