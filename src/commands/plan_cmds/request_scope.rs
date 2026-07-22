use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::cli::PlanConvergeArgs;
use crate::core::convergence::{ConvergenceInputDirection, ConvergenceRequestScope};

use super::{CommandFailure, ErrorCode, plan_failure};

pub(crate) fn convergence_request_scope(
    args: &PlanConvergeArgs,
    workspace: Option<&Path>,
) -> ConvergenceRequestScope {
    ConvergenceRequestScope {
        skill: args.skill.clone(),
        direction: if args.from_projection {
            ConvergenceInputDirection::Projection
        } else {
            ConvergenceInputDirection::Source
        },
        instance: args.instance.clone(),
        agent: args.agent.map(|agent| agent.as_str().to_string()),
        workspace_argument: args
            .workspace
            .as_ref()
            .map(|path| path.display().to_string()),
        workspace: workspace.map(|path| path.display().to_string()),
        profile: args.profile.clone(),
        require_runtime: args.require_runtime,
        accept_restart_required: args.accept_restart_required,
        push_remote: args.push_remote,
    }
}

pub(super) fn validate_convergence_request_scope(
    plan: &Value,
    request_input: Option<&Value>,
    cursor: usize,
) -> std::result::Result<(), CommandFailure> {
    let request = request_input
        .and_then(|input| input.pointer("/command/Plan/command/Converge"))
        .and_then(Value::as_object)
        .ok_or_else(|| request_evidence_failure(cursor, "missing original request evidence"))?;
    let sealed = serde_json::from_value::<ConvergenceRequestScope>(plan["request_scope"].clone())
        .map_err(|_| {
        request_evidence_failure(cursor, "invalid digest-covered request scope")
    })?;
    let started_digest = request
        .get("request_scope_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| request_evidence_failure(cursor, "invalid request_scope_digest evidence"))?;
    let sealed_digest = sealed.digest().map_err(|_| {
        request_evidence_failure(cursor, "unencodable digest-covered request scope")
    })?;
    if started_digest != sealed_digest || !request_scope_matches_plan(&sealed, plan) {
        return Err(request_scope_drift(cursor));
    }
    Ok(())
}

fn request_scope_matches_plan(scope: &ConvergenceRequestScope, plan: &Value) -> bool {
    let axes_match = plan["required_axes"].as_array().is_some_and(|axes| {
        let has_axis = |axis: &str| axes.iter().any(|value| value.as_str() == Some(axis));
        has_axis("visibility") == scope.require_runtime
            && has_axis("registry_transport") == scope.push_remote
    });
    let visibility_matches = plan["visibility"].as_array().is_some_and(|items| {
        items
            .iter()
            .all(|item| item["required"].as_bool() == Some(scope.require_runtime))
    });
    let optional = |pointer: &str| plan.pointer(pointer).and_then(Value::as_str);
    let planned_agents = plan["projections"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["agent"].as_str())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let resolved_agent = scope.agent.as_deref().or_else(|| {
        (scope.require_runtime && planned_agents.len() == 1)
            .then(|| planned_agents.iter().next().copied())
            .flatten()
    });
    axes_match
        && visibility_matches
        && plan["skill"].as_str() == Some(scope.skill.as_str())
        && optional("/source/direction")
            == Some(match scope.direction {
                ConvergenceInputDirection::Source => "source",
                ConvergenceInputDirection::Projection => "projection",
            })
        && optional("/source/input_instance") == scope.instance.as_deref()
        && optional("/selectors/input_instance") == scope.instance.as_deref()
        && optional("/selectors/agent") == resolved_agent
        && optional("/selectors/workspace") == scope.workspace.as_deref()
        && optional("/selectors/profile") == scope.profile.as_deref()
        && plan["accept_restart_required"].as_bool() == Some(scope.accept_restart_required)
        && (plan["remote"].as_str() == Some("push")) == scope.push_remote
}

fn request_scope_drift(cursor: usize) -> CommandFailure {
    plan_failure(
        ErrorCode::DependencyConflict,
        "stored convergence scope does not match the original reviewed request",
        "PLAN_REQUEST_SCOPE_DRIFT",
        false,
        vec!["create and review a fresh convergence plan".to_string()],
        Some(cursor),
    )
}

fn request_evidence_failure(cursor: usize, detail: &str) -> CommandFailure {
    plan_failure(
        ErrorCode::StateCorrupt,
        format!("stored convergence plan has {detail}"),
        "PLAN_REQUEST_EVIDENCE_INVALID",
        false,
        vec!["discard the corrupted plan and create a fresh convergence plan".to_string()],
        Some(cursor),
    )
}
