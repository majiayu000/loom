use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::surface_check::{extract_loom_commands, normalize_command};
use super::{InventoryError, NextActionEmitter, NextActionShape, validate_public_argv};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextActionTraceReport {
    pub record_count: usize,
    pub emitter_count: usize,
    pub command_count: usize,
}

pub fn check_next_action_trace(
    path: &Path,
    emitters: &[NextActionEmitter],
) -> Result<NextActionTraceReport, InventoryError> {
    let source = fs::read_to_string(path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))?;
    let mut emitter_ids = BTreeSet::new();
    let inventory = emitters
        .iter()
        .map(|emitter| (emitter.id.as_str(), emitter))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut observed_fixtures = BTreeSet::new();
    let mut shape_proven = BTreeSet::new();
    let mut record_count = 0;
    let mut command_count = 0;
    for (offset, line) in source.lines().enumerate() {
        let line_number = offset + 1;
        let record = serde_json::from_str::<Value>(line).map_err(|error| {
            InventoryError::new(format!(
                "{}:{line_number}: invalid next-action trace JSON: {error}",
                path.display()
            ))
        })?;
        let emitter_id = record["emitter_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                InventoryError::new(format!(
                    "{}:{line_number}: trace emitter_id must be a non-empty string",
                    path.display()
                ))
            })?;
        let payload = record.get("payload").ok_or_else(|| {
            InventoryError::new(format!(
                "{}:{line_number}: trace record '{emitter_id}' is missing payload",
                path.display()
            ))
        })?;
        let emitter = inventory.get(emitter_id).ok_or_else(|| {
            InventoryError::new(format!(
                "{}:{line_number}: trace contains unknown emitter '{emitter_id}'",
                path.display()
            ))
        })?;
        let fixture_id = record["fixture_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                InventoryError::new(format!(
                    "{}:{line_number}: trace fixture_id must be a non-empty string",
                    path.display()
                ))
            })?;
        let payload_type = record["payload_type"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                InventoryError::new(format!(
                    "{}:{line_number}: trace payload_type must be a non-empty string",
                    path.display()
                ))
            })?;
        if validate_payload_shape(path, line_number, emitter, payload, payload_type)? {
            shape_proven.insert(emitter_id.to_string());
        }
        if emitter.fixture_ids.iter().any(|id| id == fixture_id) {
            observed_fixtures.insert((emitter_id.to_string(), fixture_id.to_string()));
        }
        emitter_ids.insert(emitter_id.to_string());
        record_count += 1;
        command_count += validate_payload_commands(path, line_number, emitter_id, payload)?;
    }
    if record_count == 0 {
        return Err(InventoryError::new(format!(
            "{}: next-action trace is empty",
            path.display()
        )));
    }
    for emitter in emitters {
        if !shape_proven.contains(&emitter.id) {
            return Err(InventoryError::new(format!(
                "{}: emitter '{}' produced no payload proving declared {:?} shape",
                path.display(),
                emitter.id,
                emitter.shape
            )));
        }
        for fixture_id in &emitter.fixture_ids {
            if !observed_fixtures.contains(&(emitter.id.clone(), fixture_id.clone())) {
                return Err(InventoryError::new(format!(
                    "{}: emitter '{}' did not produce declared fixture '{}'",
                    path.display(),
                    emitter.id,
                    fixture_id
                )));
            }
        }
    }
    Ok(NextActionTraceReport {
        record_count,
        emitter_count: emitter_ids.len(),
        command_count,
    })
}

fn validate_payload_shape(
    path: &Path,
    line_number: usize,
    emitter: &NextActionEmitter,
    payload: &Value,
    payload_type: &str,
) -> Result<bool, InventoryError> {
    let items = payload.as_array().ok_or_else(|| {
        InventoryError::new(format!(
            "{}:{line_number}: emitter '{}' payload must be an array",
            path.display(),
            emitter.id
        ))
    })?;
    let proven = match emitter.shape {
        NextActionShape::String => {
            if !items.iter().all(Value::is_string) {
                false
            } else {
                !items.is_empty()
                    || payload_type.contains("String")
                    || payload_type.contains("&str")
            }
        }
        NextActionShape::Object => {
            if !items.iter().all(Value::is_object) {
                false
            } else {
                !items.is_empty() || payload_type.contains("NextAction")
            }
        }
    };
    let invalid_nonempty = !items.is_empty() && !proven;
    if invalid_nonempty {
        return Err(InventoryError::new(format!(
            "{}:{line_number}: emitter '{}' payload does not match declared {:?} shape (type {payload_type})",
            path.display(),
            emitter.id,
            emitter.shape
        )));
    }
    Ok(proven)
}

fn validate_payload_commands(
    path: &Path,
    line_number: usize,
    emitter_id: &str,
    value: &Value,
) -> Result<usize, InventoryError> {
    match value {
        Value::String(text) => {
            let mut count = 0;
            for command in extract_loom_commands(text) {
                let argv = normalize_command(&command);
                validate_public_argv(&argv).map_err(|error| {
                    InventoryError::new(format!(
                        "{}:{line_number}: emitter '{emitter_id}' has invalid public command {argv:?} ({:?}): {}",
                        path.display(), error.kind, error.message
                    ))
                })?;
                count += 1;
            }
            Ok(count)
        }
        Value::Array(items) => items.iter().try_fold(0, |count, item| {
            validate_payload_commands(path, line_number, emitter_id, item).map(|next| count + next)
        }),
        Value::Object(object) => object.values().try_fold(0, |count, item| {
            validate_payload_commands(path, line_number, emitter_id, item).map(|next| count + next)
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(0),
    }
}
