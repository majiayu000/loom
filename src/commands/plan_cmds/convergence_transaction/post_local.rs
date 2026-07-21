use std::path::Path;

use serde_json::{Value, json};

use super::super::super::convergence_status::{ConvergenceRequest, collect_convergence_status};
use super::super::super::helpers::{map_io, shell_arg};
use super::super::super::sync_cmds::sync_push_convergence_internal;
use super::*;
use crate::core::convergence::{ConvergenceAxis, RemotePolicy, SkillConvergencePlan};

pub(super) fn collect_local_axes(
    app: &App,
    plan: &SkillConvergencePlan,
) -> std::result::Result<Value, CommandFailure> {
    let workspace = plan.selectors.workspace.as_deref().map(Path::new);
    let collected = collect_convergence_status(
        &app.ctx,
        ConvergenceRequest {
            skill: Some(&plan.skill),
            agent: plan.selectors.agent.as_deref(),
            workspace,
            profile: plan.selectors.profile.as_deref(),
        },
    );
    let status = serde_json::to_value(collected.status).map_err(map_io)?;
    let mut visibility = status["visibility"].clone();
    if visibility["state"] == json!("visible")
        && !plan.projections.is_empty()
        && adapter_requires_reload_after_apply(&visibility)
    {
        visibility["state"] = json!("restart_required");
        visibility["evidence"]["reason"] = json!("adapter_reload_required_after_apply");
    }
    Ok(json!({
        "projections": status["projections"],
        "visibility": visibility,
    }))
}

pub(super) fn complete(
    app: &App,
    plan: &SkillConvergencePlan,
    key_digest: &str,
    mut local: Value,
) -> std::result::Result<Value, CommandFailure> {
    let evidence = local.get("evidence").cloned().unwrap_or(Value::Null);
    let projections = evidence["projections"].clone();
    let visibility = evidence["visibility"].clone();
    let mut blockers = Vec::new();
    let mut next_actions = Vec::new();

    let registry_transport = match plan.remote {
        RemotePolicy::NotRequested => json!({
            "state": "not_requested",
            "evidence": {"policy": "not_requested"},
            "errors": [],
        }),
        RemotePolicy::Push => match sync_push_convergence_internal(&app.ctx) {
            Ok(result) => json!({
                "state": "SYNCED",
                "evidence": {"result": result},
                "errors": [],
            }),
            Err(error) => {
                blockers.push("registry.remote_pending");
                next_actions.push(json!({
                    "cmd": retry_command(app, plan),
                    "reason": "retry the pending registry transport with the same immutable plan and idempotency key",
                    "idempotency_key_digest": key_digest,
                }));
                json!({
                    "state": "PENDING_PUSH",
                    "evidence": {"requested": true},
                    "errors": [{"code": error.code.as_str(), "message": error.message}],
                })
            }
        },
    };

    let visibility_state = visibility["state"].as_str();
    let visibility_evidence_usable = axis_evidence_is_usable(&visibility);
    let visibility_required = plan.required_axes.contains(&ConvergenceAxis::Visibility);
    let restart_required = visibility_state == Some("restart_required");
    if visibility_required {
        match (visibility_evidence_usable, visibility_state) {
            (true, Some("visible")) => {}
            (true, Some("restart_required")) if plan.accept_restart_required => {}
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
    let outcome = if remote_pending && restart_blocked {
        "local_complete_remote_pending_restart_required"
    } else if remote_pending {
        "local_complete_remote_pending"
    } else if restart_blocked {
        "local_complete_restart_required"
    } else if !complete {
        "local_complete_evidence_incomplete"
    } else if restart_required && plan.accept_restart_required {
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
    if plan.remote == RemotePolicy::Push {
        let remote_evidence = local["convergence"]["registry_transport"].clone();
        if let Some(evidence) = local.get_mut("evidence") {
            evidence["remote"] = remote_evidence;
        }
    }
    Ok(local)
}

fn adapter_requires_reload_after_apply(visibility: &Value) -> bool {
    visibility["evidence"]["report"]["checks"]
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

fn projection_evidence_is_complete(plan: &SkillConvergencePlan, projections: &Value) -> bool {
    if !axis_evidence_is_usable(projections) {
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
    local["convergence_id"].as_str().is_some()
        && local["plan_digest"].as_str() == Some(plan.plan_digest.as_str())
        && local["idempotency_binding_digest"].as_str().is_some()
        && local["skill"].as_str() == Some(plan.skill.as_str())
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
