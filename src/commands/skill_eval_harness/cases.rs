use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::types::ErrorCode;

use super::report::{harness_ratio, harness_schema_failure};
use crate::commands::CommandFailure;

#[derive(Debug)]
pub(crate) struct HarnessJsonlRecord<T> {
    pub(crate) line: usize,
    pub(crate) value: T,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HarnessTriggerCase {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default, alias = "input", alias = "text")]
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) expected_trigger: Option<bool>,
    #[serde(default)]
    pub(crate) should_trigger: Option<bool>,
    #[serde(default)]
    pub(crate) expected: Option<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) observed_trigger: Option<bool>,
    #[serde(default)]
    pub(crate) actual_trigger: Option<bool>,
}

impl HarnessTriggerCase {
    pub(crate) fn expected_trigger(&self) -> Option<bool> {
        self.expected_trigger
            .or(self.should_trigger)
            .or_else(|| harness_trigger_label(self.expected.as_deref()))
            .or_else(|| harness_trigger_label(self.label.as_deref()))
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct HarnessTaskCase {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default, alias = "input", alias = "prompt")]
    _task: Option<String>,
    #[serde(default)]
    pub(crate) workspace_fixture: Option<String>,
    #[serde(default)]
    pub(crate) checks: HarnessTaskChecks,
}

impl HarnessTaskCase {
    pub(crate) fn id(&self) -> String {
        self.id.clone().unwrap_or_else(|| "task".to_string())
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct HarnessTaskChecks {
    #[serde(default)]
    pub(crate) outcome_contains: Vec<String>,
    #[serde(default)]
    pub(crate) commands_contains: Vec<String>,
    #[serde(default)]
    pub(crate) max_tokens: Option<u64>,
    #[serde(default)]
    pub(crate) max_commands: Option<u64>,
    #[serde(default)]
    pub(crate) exit_code: Option<i32>,
    #[serde(default)]
    pub(crate) files_changed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct HarnessTriggerResult {
    pub(crate) id: String,
    pub(crate) line: usize,
    pub(crate) agent: String,
    pub(crate) attempt: u32,
    pub(crate) prompt: String,
    pub(crate) expected_trigger: bool,
    pub(crate) observed_trigger: bool,
    pub(crate) status: &'static str,
}

pub(crate) fn read_harness_jsonl<T: for<'de> Deserialize<'de>>(
    path: &Path,
) -> std::result::Result<Vec<HarnessJsonlRecord<T>>, CommandFailure> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            let mut failure = CommandFailure::new(
                ErrorCode::IoError,
                format!("failed to read eval file '{}': {err}", path.display()),
            );
            failure.details = json!({"path": path.display().to_string()});
            return Err(failure);
        }
    };
    let mut records = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<T>(trimmed).map_err(|err| {
            harness_schema_failure(
                format!(
                    "invalid eval JSON in '{}' at line {}: {err}",
                    path.display(),
                    line_no
                ),
                path,
                line_no,
            )
        })?;
        records.push(HarnessJsonlRecord {
            line: line_no,
            value,
        });
    }
    Ok(records)
}

pub(crate) fn evaluate_trigger_case(
    skill: &str,
    agent: &str,
    attempt: u32,
    record: &HarnessJsonlRecord<HarnessTriggerCase>,
) -> std::result::Result<HarnessTriggerResult, CommandFailure> {
    let expected = record.value.expected_trigger().ok_or_else(|| {
        harness_schema_failure(
            "trigger eval case requires expected_trigger, should_trigger, expected, or label",
            Path::new("evals/triggers.jsonl"),
            record.line,
        )
    })?;
    let prompt = record.value.prompt.as_ref().ok_or_else(|| {
        harness_schema_failure(
            "trigger eval case requires an input, prompt, or text field",
            Path::new("evals/triggers.jsonl"),
            record.line,
        )
    })?;
    let observed = record
        .value
        .observed_trigger
        .or(record.value.actual_trigger)
        .unwrap_or_else(|| infer_harness_trigger(skill, prompt));
    Ok(HarnessTriggerResult {
        id: record
            .value
            .id
            .clone()
            .unwrap_or_else(|| format!("trigger-{}", record.line)),
        line: record.line,
        agent: agent.to_string(),
        attempt,
        prompt: "<redacted>".to_string(),
        expected_trigger: expected,
        observed_trigger: observed,
        status: if expected == observed {
            "passed"
        } else {
            "failed"
        },
    })
}

pub(crate) fn summarize_trigger_results(results: &[HarnessTriggerResult]) -> Value {
    json!({
        "case_count": results.len(),
        "passed": results.iter().filter(|case| case.status == "passed").count(),
        "failed": results.iter().filter(|case| case.status == "failed").count(),
        "trigger_precision": trigger_precision(results),
        "trigger_recall": trigger_recall(results),
    })
}

pub(crate) fn trigger_precision(results: &[HarnessTriggerResult]) -> Option<f64> {
    let tp = results
        .iter()
        .filter(|case| case.expected_trigger && case.observed_trigger)
        .count();
    let fp = results
        .iter()
        .filter(|case| !case.expected_trigger && case.observed_trigger)
        .count();
    harness_ratio(tp, tp + fp)
}

pub(crate) fn trigger_recall(results: &[HarnessTriggerResult]) -> Option<f64> {
    let tp = results
        .iter()
        .filter(|case| case.expected_trigger && case.observed_trigger)
        .count();
    let fn_count = results
        .iter()
        .filter(|case| case.expected_trigger && !case.observed_trigger)
        .count();
    harness_ratio(tp, tp + fn_count)
}

pub(crate) fn harness_trigger_label(label: Option<&str>) -> Option<bool> {
    match label?.trim().to_ascii_lowercase().as_str() {
        "positive" | "trigger" | "triggers" | "true" | "should_trigger" => Some(true),
        "negative" | "ignore" | "ignored" | "false" | "should_not_trigger" => Some(false),
        _ => None,
    }
}

pub(crate) fn infer_harness_trigger(skill: &str, prompt: &str) -> bool {
    let prompt = prompt.to_ascii_lowercase();
    if prompt.contains(&skill.to_ascii_lowercase()) {
        return true;
    }
    skill
        .split(['-', '_', ' '])
        .filter(|part| part.len() >= 3)
        .any(|part| prompt.contains(&part.to_ascii_lowercase()))
}
