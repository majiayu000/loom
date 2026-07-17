use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::InventoryError;
use crate::{
    envelope::{Envelope, Meta},
    error_actions::NextAction,
    types::{ErrorCode, SyncState},
};

const SEMANTIC_CAPABILITIES: [&str; 4] = [
    "semantic:durable_plan_requires_safe_to_apply",
    "semantic:mutation_requires_compatible_contract",
    "semantic:preflight_requires_safe_to_run",
    "semantic:success_requires_ok_true",
];

pub(super) fn public_agent_capabilities() -> Result<BTreeSet<String>, InventoryError> {
    let success = Envelope::ok(
        "fixture.success",
        "req-success".to_string(),
        json!({}),
        Meta {
            warnings: vec!["fixture".to_string()],
            sync_state: Some(SyncState::Synced),
            op_id: Some("op-fixture".to_string()),
        },
    );
    let failure = Envelope::err_with_next_actions(
        "fixture.failure",
        "req-failure".to_string(),
        ErrorCode::ArgInvalid,
        "fixture failure",
        json!({"fixture": true}),
        vec![NextAction::new("loom workspace status", "inspect state")],
    );
    let samples = [serde_json::to_value(success), serde_json::to_value(failure)];
    let mut shapes = BTreeMap::<String, (BTreeSet<String>, usize)>::new();
    for sample in samples {
        let value = sample.map_err(|error| {
            InventoryError::new(format!(
                "serialize public envelope capability fixture: {error}"
            ))
        })?;
        collect_shapes("envelope", &value, &mut shapes);
    }

    let mut capabilities = BTreeSet::new();
    for (path, (kinds, sample_count)) in shapes {
        let kind = capability_kind(&path, &kinds, sample_count)?;
        capabilities.insert(format!("field:{path}:{kind}"));
    }
    capabilities.extend(SEMANTIC_CAPABILITIES.into_iter().map(str::to_string));
    Ok(capabilities)
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
    if sample_count < 2 && !conditionally_nested {
        Ok(format!("optional-{kind}"))
    } else {
        Ok(kind)
    }
}
