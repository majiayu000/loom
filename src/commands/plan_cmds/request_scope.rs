use serde_json::Value;

use super::{CommandFailure, ErrorCode, plan_failure};

pub(super) fn validate_convergence_request_scope(
    plan: &Value,
    request_input: Option<&Value>,
    cursor: usize,
) -> std::result::Result<(), CommandFailure> {
    let request = request_input
        .and_then(|input| input.pointer("/command/Plan/command/Converge"))
        .and_then(Value::as_object)
        .ok_or_else(|| request_evidence_failure(cursor, "missing original request evidence"))?;
    let request_bool = |field: &str| {
        request
            .get(field)
            .and_then(Value::as_bool)
            .ok_or_else(|| request_evidence_failure(cursor, &format!("invalid {field} evidence")))
    };
    let require_runtime = request_bool("require_runtime")?;
    let accept_restart_required = request_bool("accept_restart_required")?;
    let push_remote = request_bool("push_remote")?;
    let axes = plan["required_axes"]
        .as_array()
        .ok_or_else(|| request_evidence_failure(cursor, "stored plan has invalid required_axes"))?;
    let has_axis = |axis: &str| axes.iter().any(|value| value.as_str() == Some(axis));
    let visibility_matches = plan["visibility"].as_array().is_some_and(|items| {
        items
            .iter()
            .all(|item| item["required"].as_bool() == Some(require_runtime))
    });

    if has_axis("visibility") != require_runtime
        || !visibility_matches
        || plan["accept_restart_required"].as_bool() != Some(accept_restart_required)
        || has_axis("registry_transport") != push_remote
        || (plan["remote"].as_str() == Some("push")) != push_remote
    {
        return Err(plan_failure(
            ErrorCode::DependencyConflict,
            "stored convergence scope does not match the original reviewed request",
            "PLAN_REQUEST_SCOPE_DRIFT",
            false,
            vec!["create and review a fresh convergence plan".to_string()],
            Some(cursor),
        ));
    }
    Ok(())
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
