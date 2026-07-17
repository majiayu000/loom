use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use serde_json::{Value, json};

use super::InventoryError;
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
    capabilities.extend(semantic_capabilities(repo_root, &serialized)?);
    Ok(capabilities)
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
    if skill.contains("stop all mutations")
        && metadata.contains("cli_contract = \">=1.0.0,<2.0.0\"")
    {
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
