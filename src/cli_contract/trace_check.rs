use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::surface_check::{extract_loom_commands, normalize_command};
use super::{InventoryError, validate_public_argv};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextActionTraceReport {
    pub record_count: usize,
    pub emitter_count: usize,
    pub command_count: usize,
}

pub fn check_next_action_trace(path: &Path) -> Result<NextActionTraceReport, InventoryError> {
    let source = fs::read_to_string(path)
        .map_err(|error| InventoryError::new(format!("{}: {error}", path.display())))?;
    let mut emitter_ids = BTreeSet::new();
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
    Ok(NextActionTraceReport {
        record_count,
        emitter_count: emitter_ids.len(),
        command_count,
    })
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
