use std::path::Path;

use serde_json::{Value, json};

use super::super::super::convergence_status::{ConvergenceRequest, collect_convergence_status};
use super::super::super::helpers::{map_io, shell_arg};
use super::super::super::sync_cmds::sync_push_convergence_internal;
use super::*;
use crate::core::convergence::{ConvergenceAxis, RemotePolicy, SkillConvergencePlan};
use crate::core::convergence_status::{
    AxisError, ConvergenceStatus, RegistryTransportState, RegistryTransportStatus, VisibilityState,
};

enum RegistryTransportOutcome {
    NotRequested,
    Synced(&'static str),
    Pending(AxisError),
}

pub(super) fn collect_local_axes(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<Value, CommandFailure> {
    let status = collect_local_status(app, plan);
    serde_json::to_value(json!({
        "projections": status.projections,
        "visibility": status.visibility,
    }))
    .map_err(map_io)
}

fn collect_local_status(app: &App, plan: &SkillConvergencePlan) -> ConvergenceStatus {
    let workspace = plan.selectors.workspace.as_deref().map(Path::new);
    let mut status = collect_convergence_status(
        &app.ctx,
        ConvergenceRequest {
            skill: Some(&plan.skill),
            agent: plan.selectors.agent.as_deref(),
            workspace,
            profile: plan.selectors.profile.as_deref(),
        },
    )
    .status;
    if status.visibility.state == VisibilityState::Visible
        && !plan.projections.is_empty()
        && adapter_requires_reload_after_apply(&status.visibility.evidence)
    {
        status.visibility.state = VisibilityState::RestartRequired;
        status.visibility.evidence["reason"] = json!("adapter_reload_required_after_apply");
    }
    status
}

pub(super) fn complete(
    app: &App,
    plan: &SkillConvergencePlan,
    key_digest: &str,
    mut local: Value,
) -> std::result::Result<Value, CommandFailure> {
    let mut blockers = Vec::new();
    let mut next_actions = Vec::new();
    let pre_transport = collect_local_status(app, plan);
    let pre_projections = serde_json::to_value(&pre_transport.projections).map_err(map_io)?;
    let pre_visibility = serde_json::to_value(&pre_transport.visibility).map_err(map_io)?;
    let local_evidence_allows_transport = projection_evidence_is_complete(plan, &pre_projections)
        && visibility_evidence_allows_transport(plan, &pre_visibility);
    let transport_outcome = match plan.remote {
        RemotePolicy::NotRequested => RegistryTransportOutcome::NotRequested,
        RemotePolicy::Push if !local_evidence_allows_transport => {
            RegistryTransportOutcome::Pending(AxisError::new(
                "local_evidence_incomplete",
                "required local convergence evidence changed before remote transport",
            ))
        }
        RemotePolicy::Push => match validate_transport_scope(app, plan, &local)
            .and_then(|()| sync_push_convergence_internal(&app.ctx))
        {
            Ok(result) => RegistryTransportOutcome::Synced(result),
            Err(error) => RegistryTransportOutcome::Pending(AxisError::new(
                error.code.as_str(),
                error.message,
            )),
        },
    };
    let remote_pending = matches!(transport_outcome, RegistryTransportOutcome::Pending(_));
    if remote_pending {
        blockers.push("registry.remote_pending");
        next_actions.push(json!({
            "cmd": retry_command(app, plan),
            "reason": "retry the pending registry transport with the same immutable plan and idempotency key",
            "idempotency_key_digest": key_digest,
        }));
    }

    let final_status = collect_local_status(app, plan);
    let projections = serde_json::to_value(final_status.projections).map_err(map_io)?;
    let visibility = serde_json::to_value(final_status.visibility).map_err(map_io)?;
    let registry_transport =
        registry_transport_axis(final_status.registry_transport, transport_outcome)?;

    if plan
        .required_axes
        .contains(&ConvergenceAxis::RegistryTransport)
        && !transport_evidence_is_complete(plan, &registry_transport)
    {
        blockers.push("registry_transport.evidence_incomplete");
    }

    let visibility_state = visibility["state"].as_str();
    let visibility_evidence_usable =
        axis_evidence_is_usable(&visibility) && axis_freshness_is_complete(plan, &visibility);
    let visibility_required = plan.required_axes.contains(&ConvergenceAxis::Visibility);
    let restart_required = visibility_state == Some("restart_required");
    let restart_accepted =
        restart_evidence_is_acceptable(plan.accept_restart_required, &visibility);
    if visibility_required {
        match (visibility_evidence_usable, visibility_state) {
            (true, Some("visible")) => {}
            (true, Some("restart_required")) if restart_accepted => {}
            (true, Some("restart_required")) => blockers.push("visibility.restart_required"),
            _ => blockers.push("visibility.evidence_incomplete"),
        }
    }
    if restart_required {
        next_actions.push(json!({
            "cmd": visibility_recheck_command(app, plan),
            "reason": "restart the affected agent runtime first, then recheck visibility",
        }));
    } else if visibility_required && visibility_state != Some("visible") {
        next_actions.push(json!({
            "cmd": visibility_recheck_command(app, plan),
            "reason": "collect successful adapter visibility evidence for the required runtime axis",
        }));
    }

    if !projection_evidence_is_complete(plan, &projections) {
        blockers.push("projections.evidence_incomplete");
        next_actions.push(json!({
            "cmd": visibility_recheck_command(app, plan),
            "reason": "inspect projection evidence before reporting convergence complete",
        }));
    }
    if !declared_local_evidence_is_complete(plan, &local) {
        blockers.push("evidence.required_missing");
        next_actions.push(json!({
            "cmd": visibility_recheck_command(app, plan),
            "reason": "inspect the retained convergence evidence before retrying",
        }));
    }

    let complete = blockers.is_empty();
    let remote_pending = blockers.contains(&"registry.remote_pending");
    let restart_blocked = blockers.contains(&"visibility.restart_required");
    let evidence_incomplete = blockers.iter().any(|blocker| {
        !matches!(
            *blocker,
            "registry.remote_pending" | "visibility.restart_required"
        )
    });
    let outcome = if evidence_incomplete {
        "local_complete_evidence_incomplete"
    } else if remote_pending && restart_blocked {
        "local_complete_remote_pending_restart_required"
    } else if remote_pending {
        "local_complete_remote_pending"
    } else if restart_blocked {
        "local_complete_restart_required"
    } else if restart_accepted {
        "complete_with_restart_required"
    } else {
        "complete"
    };
    let source = json!({
        "commit": local["source_commit"],
        "direction": plan.source.direction,
    });
    let convergence = json!({
        "registry_transport": registry_transport,
        "projections": projections,
        "visibility": visibility,
    });
    local["local_state"] = json!("complete");
    local["outcome"] = json!(outcome);
    local["completion_blockers"] = json!(blockers);
    local["source"] = source;
    local["convergence"] = convergence;
    local["complete"] = json!(complete);
    local["next_actions"] = json!(next_actions);
    Ok(local)
}

fn validate_transport_scope(
    app: &App,
    plan: &SkillConvergencePlan,
    local: &Value,
) -> std::result::Result<(), CommandFailure> {
    let status = gitops::run_git(
        &app.ctx,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            ".gitignore",
            ".gitattributes",
            "state/registry",
            "state/v3",
        ],
    )
    .map_err(map_git)?;
    if !status.is_empty() {
        let mut failure = CommandFailure::new(
            ErrorCode::DependencyConflict,
            "unplanned registry transport paths changed after convergence planning",
        );
        failure.details = json!({
            "conflict": {
                "code": "CONVERGENCE_TRANSPORT_SCOPE_DRIFT",
                "paths": status.lines().collect::<Vec<_>>(),
            }
        });
        return Err(failure);
    }
    require_exact_transport_boundary(app, plan, local)
}

pub(super) fn require_exact_transport_boundary(
    app: &App,
    plan: &SkillConvergencePlan,
    local: &Value,
) -> std::result::Result<(), CommandFailure> {
    let expected_head = recorded_final_commit(plan, local)?;
    let live_head = gitops::head(&app.ctx).map_err(map_git)?;
    if live_head == expected_head {
        return Ok(());
    }
    let expected_is_ancestor = gitops::run_git_allow_failure(
        &app.ctx,
        &["merge-base", "--is-ancestor", expected_head, &live_head],
    )
    .map_err(map_git)?
    .status
    .success();
    let (code, message) = if expected_is_ancestor {
        (
            "CONVERGENCE_COMMIT_BOUNDARY_DRIFT",
            "live HEAD moved past the exact convergence commit boundary",
        )
    } else {
        (
            "CONVERGENCE_COMMIT_EVIDENCE_STALE",
            "live HEAD no longer contains the recorded convergence commit boundary",
        )
    };
    let mut failure = CommandFailure::new(ErrorCode::DependencyConflict, message);
    failure.details = json!({
        "conflict": {
            "code": code,
            "expected_head": expected_head,
            "live_head": live_head,
        }
    });
    Err(failure)
}

fn recorded_final_commit<'a>(
    plan: &'a SkillConvergencePlan,
    local: &'a Value,
) -> std::result::Result<&'a str, CommandFailure> {
    match &local["registry_commit"] {
        Value::String(commit) => return Ok(commit),
        Value::Null => {}
        _ => {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "local convergence evidence has an invalid registry commit",
            ));
        }
    }
    match &local["source_commit"] {
        Value::String(commit) => Ok(commit),
        Value::Null => Ok(&plan.source.registry_head),
        _ => Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "local convergence evidence has an invalid source commit",
        )),
    }
}

fn registry_transport_axis(
    mut status: RegistryTransportStatus,
    outcome: RegistryTransportOutcome,
) -> std::result::Result<Value, CommandFailure> {
    status.evidence["observed_at"] = json!(status.observed_at);
    let collected_has_errors = !status.errors.is_empty();
    let mut freshness_errors = status
        .errors
        .into_iter()
        .filter(|error| error.code == "evidence_changed_during_read")
        .collect::<Vec<_>>();
    match outcome {
        RegistryTransportOutcome::NotRequested => {
            status.state = RegistryTransportState::NotRequested;
            status.evidence["policy"] = json!("not_requested");
        }
        RegistryTransportOutcome::Synced(result) => {
            if status.state != RegistryTransportState::Synced
                || collected_has_errors
                || !freshness_errors.is_empty()
                || status.stale
            {
                status.state = RegistryTransportState::Error;
                freshness_errors.push(AxisError::new(
                    "transport_postcondition_failed",
                    "registry transport status did not confirm the completed push",
                ));
            }
            status.evidence["result"] = json!(result);
        }
        RegistryTransportOutcome::Pending(error) => {
            status.state = RegistryTransportState::PendingPush;
            status.evidence["requested"] = json!(true);
            freshness_errors.push(error);
        }
    }
    status.errors = freshness_errors;
    serde_json::to_value(status).map_err(map_io)
}

fn transport_evidence_is_complete(plan: &SkillConvergencePlan, axis: &Value) -> bool {
    axis_freshness_is_complete(plan, axis) && axis["state"] != json!("ERROR")
}

fn adapter_requires_reload_after_apply(visibility: &Value) -> bool {
    visibility["report"]["checks"]
        .as_array()
        .is_some_and(|checks| {
            checks
                .iter()
                .any(|check| check["details"]["restart_required_after_apply"] == json!(true))
        })
}

fn axis_evidence_is_usable(axis: &Value) -> bool {
    axis["stale"] == json!(false)
        && axis["errors"]
            .as_array()
            .is_some_and(|errors| errors.is_empty())
}

fn axis_freshness_is_complete(plan: &SkillConvergencePlan, axis: &Value) -> bool {
    axis["stale"] == json!(false)
        && axis["observed_at"].as_str().is_some()
        && axis["evidence"]["observed_revision"].as_str().is_some()
        && (!plan.registry.initialized
            || axis["evidence"]["checkpoint_updated_at"].as_str().is_some())
}

fn visibility_evidence_allows_transport(plan: &SkillConvergencePlan, visibility: &Value) -> bool {
    !plan.required_axes.contains(&ConvergenceAxis::Visibility)
        || (axis_evidence_is_usable(visibility)
            && axis_freshness_is_complete(plan, visibility)
            && (visibility["state"] == json!("visible")
                || restart_evidence_is_acceptable(plan.accept_restart_required, visibility)))
}

fn restart_evidence_is_acceptable(accepted: bool, visibility: &Value) -> bool {
    accepted
        && visibility["state"] == json!("restart_required")
        && visibility["evidence"]["report"]["visible"] == json!(true)
}

fn projection_evidence_is_complete(plan: &SkillConvergencePlan, projections: &Value) -> bool {
    if !axis_evidence_is_usable(projections) || !axis_freshness_is_complete(plan, projections) {
        return false;
    }
    match projections["state"].as_str() {
        Some("converged") => {
            exact_projection_evidence_matches(&plan.skill, &plan.projections, projections)
        }
        Some("not_applicable") => {
            plan.projections.is_empty()
                && projections["items"]
                    .as_array()
                    .is_some_and(|items| items.is_empty())
                && projections["evidence"]["selected_count"] == json!(0)
        }
        _ => false,
    }
}

fn exact_projection_evidence_matches(
    skill: &str,
    effects: &[crate::core::convergence::ProjectionEffectPlan],
    projections: &Value,
) -> bool {
    let Some(items) = projections["items"].as_array() else {
        return false;
    };
    if effects.is_empty()
        || items.len() != effects.len()
        || projections["evidence"]["selected_count"] != json!(effects.len())
    {
        return false;
    }
    effects.iter().all(|effect| {
        let mut matching = items
            .iter()
            .filter(|item| item["instance_id"].as_str() == Some(effect.instance_id.as_str()))
            .take(2);
        matching.next().is_some_and(|item| {
            item["skill_id"].as_str() == Some(skill)
                && item["target_id"].as_str() == Some(effect.target_id.as_str())
                && item["method"].as_str() == Some(effect.method.as_str())
                && item["state"] == json!("converged")
                && item["errors"]
                    .as_array()
                    .is_some_and(|errors| errors.is_empty())
                && if effect.method == "symlink" {
                    item["source_digest"].is_null() && item["materialized_digest"].is_null()
                } else {
                    item["source_digest"].as_str() == Some(effect.source_tree_digest.as_str())
                        && item["materialized_digest"].as_str()
                            == Some(effect.source_tree_digest.as_str())
                }
        }) && matching.next().is_none()
    })
}

fn declared_local_evidence_is_complete(plan: &SkillConvergencePlan, local: &Value) -> bool {
    local["skill"].as_str() == Some(plan.skill.as_str())
        && local["evidence"]["source"]["direction"].as_str().is_some()
        && local["evidence"]["projections"].is_object()
        && local["evidence"]["registry_operation"].is_object()
        && local["evidence"]["visibility"].is_object()
        && local["evidence"]["remote"].is_object()
        && local["evidence"]["recovery"].is_object()
}

pub(super) fn retry_evidence_is_valid(plan: &SkillConvergencePlan, local: &Value) -> bool {
    local["local_state"] == json!("complete") && declared_local_evidence_is_complete(plan, local)
}

fn retry_command(app: &App, plan: &SkillConvergencePlan) -> String {
    format!(
        "loom --json --root {} apply {} --plan-digest {} --idempotency-key \"$IDEMPOTENCY_KEY\"",
        shell_arg(&app.ctx.root),
        plan.plan_id,
        plan.plan_digest,
    )
}

fn visibility_recheck_command(app: &App, plan: &SkillConvergencePlan) -> String {
    let mut command = format!(
        "loom --json --root {} skill inspect {}",
        shell_arg(&app.ctx.root),
        plan.skill,
    );
    if let Some(agent) = plan.selectors.agent.as_deref() {
        command.push_str(&format!(" --agent {agent}"));
    }
    command
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::core::convergence::ProjectionEffectPlan;

    fn effect(instance_id: &str, target_id: &str) -> ProjectionEffectPlan {
        ProjectionEffectPlan {
            instance_id: instance_id.to_string(),
            binding_id: format!("binding-{instance_id}"),
            target_id: target_id.to_string(),
            agent: "codex".to_string(),
            profile: "default".to_string(),
            method: "copy".to_string(),
            ownership: "managed".to_string(),
            materialized_path: format!("/target/{instance_id}"),
            source_tree_digest: "source-digest".to_string(),
            materialized_tree_digest: Some("old-digest".to_string()),
            effect: "refresh".to_string(),
        }
    }

    fn item(instance_id: &str, target_id: &str) -> Value {
        json!({
            "instance_id": instance_id,
            "skill_id": "demo",
            "target_id": target_id,
            "method": "copy",
            "state": "converged",
            "source_digest": "source-digest",
            "materialized_digest": "source-digest",
            "errors": [],
        })
    }

    #[test]
    fn stale_or_error_visibility_evidence_is_not_usable() {
        assert!(!axis_evidence_is_usable(&json!({
            "state": "visible",
            "stale": true,
            "errors": [{"code": "evidence_changed_during_read"}],
        })));
        assert!(!axis_evidence_is_usable(&json!({
            "state": "visible",
            "stale": false,
            "errors": [{"code": "adapter_failed"}],
        })));
        assert!(axis_evidence_is_usable(&json!({
            "state": "visible",
            "stale": false,
            "errors": [],
        })));
    }

    #[test]
    fn restart_acceptance_requires_an_actually_visible_report() {
        assert!(restart_evidence_is_acceptable(
            true,
            &json!({"state": "restart_required", "evidence": {"report": {"visible": true}}})
        ));
        assert!(!restart_evidence_is_acceptable(
            true,
            &json!({"state": "restart_required", "evidence": {"report": {"visible": false}}})
        ));
        assert!(!restart_evidence_is_acceptable(
            false,
            &json!({"state": "restart_required", "evidence": {"report": {"visible": true}}})
        ));
    }

    #[test]
    fn projection_evidence_requires_every_exact_planned_effect() {
        let effects = vec![
            effect("projection-a", "target-a"),
            effect("projection-b", "target-b"),
        ];
        let omitted = json!({
            "evidence": {"selected_count": 1},
            "items": [item("projection-a", "target-a")],
        });
        assert!(!exact_projection_evidence_matches(
            "demo", &effects, &omitted
        ));

        let exact = json!({
            "evidence": {"selected_count": 2},
            "items": [item("projection-a", "target-a"), item("projection-b", "target-b")],
        });
        assert!(exact_projection_evidence_matches("demo", &effects, &exact));

        let mut wrong_method = exact;
        wrong_method["items"][1]["method"] = json!("symlink");
        assert!(!exact_projection_evidence_matches(
            "demo",
            &effects,
            &wrong_method
        ));
    }
}
