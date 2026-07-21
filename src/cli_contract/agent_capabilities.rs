use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde_json::{Value, json};

use super::{
    CLI_CONTRACT_VERSION, InventoryError,
    contract_policy::{contract_range, ensure_contract_range_contains_version},
};
use crate::{
    envelope::{Envelope, Meta},
    error_actions::NextAction,
    types::{ErrorCode, SyncState},
};

pub(super) fn public_agent_capabilities(
    repo_root: &Path,
) -> Result<BTreeSet<String>, InventoryError> {
    let preflight = Envelope::ok(
        "agent.preflight",
        "req-preflight".to_string(),
        json!({"safe_to_run": true}),
        Meta {
            warnings: vec!["fixture".to_string()],
            sync_state: Some(SyncState::Synced),
            op_id: Some("op-fixture".to_string()),
        },
    );
    let durable_plan = Envelope::ok(
        "plan.use",
        "req-plan".to_string(),
        json!({"safe_to_apply": true, "required_approvals": ["approval-fixture"]}),
        Meta::default(),
    );
    let convergence_plan = Envelope::ok(
        "plan.converge",
        "req-convergence-plan".to_string(),
        json!({
            "input": {},
            "preflight": {},
            "input_conflicts": [{}],
        }),
        Meta::default(),
    );
    let convergence_apply = Envelope::ok(
        "apply",
        "req-convergence-apply".to_string(),
        json!({
            "local_state": "complete",
            "outcome": "local_complete_remote_pending_restart_required",
            "completion_blockers": ["registry.remote_pending", "visibility.restart_required"],
            "source": {"commit": "commit-fixture", "direction": "source"},
            "convergence": {
                "registry_transport": {
                    "state": "PENDING_PUSH",
                    "evidence": {"requested": true},
                    "errors": [{"code": "PUSH_REJECTED", "message": "fixture"}],
                },
                "projections": {
                    "state": "converged",
                    "items": [{
                        "instance_id": "projection-fixture",
                        "skill_id": "skill-fixture",
                        "target_id": "target-fixture",
                        "method": "copy",
                        "state": "converged",
                        "source_digest": "digest-fixture",
                        "materialized_digest": "digest-fixture",
                        "errors": [{"code": "fixture_item_error", "message": "fixture"}],
                    }],
                    "evidence": {"selected_count": 1},
                    "stale": false,
                    "errors": [{"code": "fixture_projection_error", "message": "fixture"}],
                },
                "visibility": {
                    "state": "restart_required",
                    "evidence": {"reason": "adapter_reload_required_after_apply"},
                    "stale": false,
                    "errors": [{"code": "fixture_visibility_error", "message": "fixture"}],
                },
            },
            "complete": false,
            "next_actions": [{"cmd": "loom skill inspect skill-fixture", "reason": "fixture"}],
        }),
        Meta::default(),
    );
    let failure = Envelope::err_with_next_actions(
        "fixture.failure",
        "req-failure".to_string(),
        ErrorCode::ArgInvalid,
        "fixture failure",
        json!({"fixture": true}),
        vec![NextAction::new("loom workspace status", "inspect state")],
    );
    let samples = [
        serde_json::to_value(preflight),
        serde_json::to_value(durable_plan),
        serde_json::to_value(convergence_plan),
        serde_json::to_value(convergence_apply),
        serde_json::to_value(failure),
    ];
    let mut shapes = BTreeMap::<String, (BTreeSet<String>, usize)>::new();
    let mut serialized = Vec::new();
    for sample in samples {
        let value = sample.map_err(|error| {
            InventoryError::new(format!(
                "serialize public envelope capability fixture: {error}"
            ))
        })?;
        collect_shapes("envelope", &value, &mut shapes);
        serialized.push(value);
    }

    let mut capabilities = BTreeSet::new();
    for (path, (kinds, sample_count)) in shapes {
        let kind = capability_kind(&path, &kinds, sample_count, serialized.len())?;
        capabilities.insert(format!("field:{path}:{kind}"));
    }
    capabilities.extend(
        ErrorCode::ALL
            .into_iter()
            .map(|code| format!("value:envelope.error.code:{}", code.as_str())),
    );
    capabilities.extend(convergence_completion_values().map(str::to_string));
    capabilities.extend(semantic_capabilities(repo_root, &serialized)?);
    Ok(capabilities)
}

fn convergence_completion_values() -> impl Iterator<Item = &'static str> {
    [
        "value:envelope.data.local_state:complete",
        "value:envelope.data.outcome:local_complete_restart_required",
        "value:envelope.data.outcome:complete_with_restart_required",
        "value:envelope.data.outcome:local_complete_remote_pending",
        "value:envelope.data.outcome:local_complete_remote_pending_restart_required",
        "value:envelope.data.outcome:local_complete_evidence_incomplete",
        "value:envelope.data.outcome:complete",
        "value:envelope.data.completion_blockers[]:registry.remote_pending",
        "value:envelope.data.completion_blockers[]:visibility.restart_required",
        "value:envelope.data.completion_blockers[]:visibility.evidence_incomplete",
        "value:envelope.data.completion_blockers[]:projections.evidence_incomplete",
        "value:envelope.data.completion_blockers[]:evidence.required_missing",
        "value:envelope.data.convergence.registry_transport.state:not_requested",
        "value:envelope.data.convergence.registry_transport.state:SYNCED",
        "value:envelope.data.convergence.registry_transport.state:PENDING_PUSH",
        "value:envelope.data.convergence.projections.state:converged",
        "value:envelope.data.convergence.projections.state:not_applicable",
        "value:envelope.data.convergence.visibility.state:visible",
        "value:envelope.data.convergence.visibility.state:restart_required",
        "value:envelope.data.convergence.visibility.state:not_visible",
        "value:envelope.data.convergence.visibility.state:unsupported",
        "value:envelope.data.convergence.visibility.state:unknown",
        "value:envelope.data.convergence.visibility.state:error",
    ]
    .into_iter()
}

fn semantic_capabilities(
    repo_root: &Path,
    samples: &[Value],
) -> Result<BTreeSet<String>, InventoryError> {
    let skill_path = repo_root.join("skills/loom-registry/SKILL.md");
    let skill = fs::read_to_string(&skill_path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", skill_path.display())))?;
    let metadata_path = repo_root.join("skills/loom-registry/loom.skill.toml");
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", metadata_path.display())))?;
    let declared_range = contract_range(&metadata, &metadata_path.display().to_string())?;
    ensure_contract_range_contains_version(&declared_range, CLI_CONTRACT_VERSION)?;
    let mut semantics = BTreeSet::new();
    if samples.first().and_then(|value| value.get("ok")) == Some(&Value::Bool(true))
        && samples.last().and_then(|value| value.get("ok")) == Some(&Value::Bool(false))
        && skill.contains("Treat only `ok=true` as success")
    {
        semantics.insert("semantic:success_requires_ok_true".to_string());
    }
    if skill.contains("Require `data.safe_to_run=true`") {
        semantics.insert("semantic:preflight_requires_safe_to_run".to_string());
    }
    if skill.contains("Require `data.safe_to_apply=true`") {
        semantics.insert("semantic:durable_plan_requires_safe_to_apply".to_string());
    }
    if skill.contains("stop all mutations") {
        semantics.insert("semantic:mutation_requires_compatible_contract".to_string());
    }
    Ok(semantics)
}

fn collect_shapes(
    prefix: &str,
    value: &Value,
    shapes: &mut BTreeMap<String, (BTreeSet<String>, usize)>,
) {
    let Value::Object(fields) = value else {
        return;
    };
    for (key, value) in fields {
        let path = format!("{prefix}.{key}");
        let shape = json_shape(value);
        let entry = shapes.entry(path.clone()).or_default();
        entry.0.insert(shape.to_string());
        entry.1 += 1;
        if value.is_object() && path != "envelope.error.details" {
            collect_shapes(&path, value, shapes);
        } else if let Value::Array(values) = value
            && values.iter().all(Value::is_object)
        {
            for value in values {
                collect_shapes(&format!("{path}[]"), value, shapes);
            }
        }
    }
}

fn json_shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(values) if values.iter().all(Value::is_string) => "array-string",
        Value::Array(values) if values.iter().all(Value::is_object) => "array-object",
        Value::Array(_) => "array-json",
        Value::Object(_) => "object",
    }
}

fn capability_kind(
    path: &str,
    kinds: &BTreeSet<String>,
    sample_count: usize,
    total_samples: usize,
) -> Result<String, InventoryError> {
    let special = match path {
        "envelope.cli_contract_version" => Some("semver-string"),
        "envelope.error" => Some("null-or-object"),
        "envelope.error.details" => Some("any-json"),
        "envelope.error.next_actions" => Some("optional-array-object"),
        "envelope.meta.sync_state" => Some("optional-enum"),
        "envelope.meta.op_id" => Some("optional-string"),
        "envelope.meta.warnings" => Some("array-string"),
        _ => None,
    };
    if let Some(special) = special {
        return Ok(special.to_string());
    }
    let mut non_null = kinds
        .iter()
        .filter(|kind| kind.as_str() != "null")
        .cloned()
        .collect::<BTreeSet<_>>();
    if kinds.contains("null") && non_null.len() == 1 {
        return Ok(format!(
            "null-or-{}",
            non_null.pop_first().expect("one non-null shape")
        ));
    }
    if non_null.len() != 1 {
        return Err(InventoryError::new(format!(
            "{path}: envelope fixture produced ambiguous field shapes {kinds:?}"
        )));
    }
    let kind = non_null.pop_first().expect("one field shape");
    let conditionally_nested = path.starts_with("envelope.error.");
    if sample_count < total_samples && !conditionally_nested {
        Ok(format!("optional-{kind}"))
    } else {
        Ok(kind)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{convergence_completion_values, public_agent_capabilities};
    use crate::types::ErrorCode;

    #[test]
    fn agent_capabilities_track_error_codes_and_next_action_fields() {
        let capabilities = public_agent_capabilities(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("repository agent capability snapshot");
        for code in ErrorCode::ALL {
            assert!(capabilities.contains(&format!("value:envelope.error.code:{}", code.as_str())));
        }
        assert!(capabilities.contains("field:envelope.error.next_actions[].cmd:string"));
        assert!(capabilities.contains("field:envelope.error.next_actions[].reason:string"));
        for field in [
            "field:envelope.data.local_state:optional-string",
            "field:envelope.data.outcome:optional-string",
            "field:envelope.data.completion_blockers:optional-array-string",
            "field:envelope.data.complete:optional-boolean",
            "field:envelope.data.convergence:optional-object",
            "field:envelope.data.convergence.registry_transport:optional-object",
            "field:envelope.data.convergence.projections:optional-object",
            "field:envelope.data.convergence.visibility:optional-object",
        ] {
            assert!(capabilities.contains(field), "missing {field}");
        }
        for value in convergence_completion_values() {
            assert!(capabilities.contains(value), "missing {value}");
        }
    }
}
