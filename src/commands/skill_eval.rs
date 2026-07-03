use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{SkillEvalArgs, SkillEvalCommand, SkillEvalOfflineArgs};
use crate::envelope::Meta;
use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

use super::skill_eval_harness::persist_report;
use super::skill_verify::{head_tree_oid_for_path, last_commit_for_path};
use super::telemetry::record_skill_eval_telemetry;
use super::{App, CommandFailure};

#[path = "skill_eval/offline.rs"]
mod offline;
pub(crate) use offline::build_skill_eval_offline_report;

const EVAL_SCHEMA_VERSION: u32 = 1;

impl App {
    pub fn cmd_skill_eval(
        &self,
        args: &SkillEvalArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match &args.command {
            Some(SkillEvalCommand::Offline(offline)) => self.cmd_skill_eval_offline(offline),
            Some(SkillEvalCommand::Run(run)) => self.cmd_skill_eval_run(run),
            Some(SkillEvalCommand::Trigger(trigger)) => self.cmd_skill_eval_trigger(trigger),
            Some(SkillEvalCommand::Compare(compare)) => self.cmd_skill_eval_compare(compare),
            None => {
                let skill = args.skill.clone().ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        "skill eval requires a skill name or an eval subcommand",
                    )
                })?;
                let offline = SkillEvalOfflineArgs {
                    skill,
                    agent: args.agent.clone(),
                    matrix: args.matrix.clone(),
                    model: args.model.clone(),
                };
                self.cmd_skill_eval_offline(&offline)
            }
        }
    }

    pub(crate) fn cmd_skill_eval_offline(
        &self,
        args: &SkillEvalOfflineArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let mut result = build_skill_eval_offline_report(&self.ctx, args)?;
        for run in &result.runs {
            record_skill_eval_telemetry(
                &self.ctx,
                &args.skill,
                Some(&run.agent),
                run.summary.failed == 0,
                Some(run.summary.token_count),
                Some(run.summary.command_count),
                None,
            )?;
        }
        persist_report(&self.ctx, &args.skill, "offline", None, &mut result.report)?;
        if result.failed > 0 {
            let mut failure = CommandFailure::new(
                ErrorCode::EvalFailed,
                format!("skill eval failed with {} failing case(s)", result.failed),
            );
            failure.details = json!({
                "failed": result.failed,
                "report": result.report,
            });
            return Err(failure);
        }

        Ok((
            result.report,
            Meta {
                warnings: result.warnings,
                ..Meta::default()
            },
        ))
    }
}

pub(crate) fn skill_eval_version(ctx: &crate::state::AppContext, skill: &str) -> SkillEvalVersion {
    let skill_rel = format!("skills/{skill}");
    SkillEvalVersion {
        head_tree_oid: head_tree_oid_for_path(ctx, &skill_rel).ok().flatten(),
        last_source_commit: last_commit_for_path(ctx, &skill_rel).ok().flatten(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SkillEvalVersion {
    pub(crate) head_tree_oid: Option<String>,
    pub(crate) last_source_commit: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TriggerCase {
    #[serde(default)]
    pub(crate) id: Option<String>,
    #[serde(default, alias = "prompt", alias = "text")]
    input: Option<String>,
    #[serde(default)]
    expected_trigger: Option<bool>,
    #[serde(default)]
    should_trigger: Option<bool>,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    observed_trigger: Option<bool>,
    #[serde(default)]
    actual_trigger: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TaskCase {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, alias = "input", alias = "prompt")]
    pub(crate) task: Option<String>,
    #[serde(default)]
    pub(crate) output: String,
    #[serde(default)]
    pub(crate) trace: Vec<String>,
    #[serde(default)]
    pub(crate) metrics: TaskMetrics,
    #[serde(default)]
    pub(crate) permissions_used: Vec<String>,
    #[serde(default)]
    pub(crate) checks: TaskChecks,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub(crate) struct TaskMetrics {
    pub(crate) tokens: Option<u64>,
    pub(crate) commands: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct TaskChecks {
    #[serde(default)]
    pub(crate) outcome_contains: Vec<String>,
    #[serde(default)]
    pub(crate) process_contains: Vec<String>,
    #[serde(default)]
    pub(crate) style_contains: Vec<String>,
    #[serde(default)]
    pub(crate) max_tokens: Option<u64>,
    #[serde(default)]
    pub(crate) max_commands: Option<u64>,
    #[serde(default)]
    artifacts: Vec<ArtifactCheck>,
}

#[derive(Debug, Deserialize)]
struct ArtifactCheck {
    path: String,
    #[serde(default)]
    sha256: Option<String>,
}

#[derive(Debug)]
pub(crate) struct JsonlRecord<T> {
    pub(crate) line: usize,
    pub(crate) value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvalStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Serialize)]
pub(crate) struct EvalRun {
    pub(crate) agent: String,
    pub(crate) model: Option<String>,
    pub(crate) mode: &'static str,
    pub(crate) summary: EvalSummary,
    pub(crate) triggers: Vec<TriggerResult>,
    pub(crate) tasks: Vec<TaskResult>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct EvalSummary {
    pub(crate) case_count: usize,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) skipped: usize,
    pub(crate) aggregate_score: Option<f64>,
    pub(crate) trigger_precision: Option<f64>,
    pub(crate) trigger_recall: Option<f64>,
    pub(crate) task_success_rate: Option<f64>,
    pub(crate) token_count: u64,
    pub(crate) command_count: u64,
    pub(crate) permissions_used: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TriggerResult {
    id: String,
    line: usize,
    prompt: String,
    expected_trigger: bool,
    observed_trigger: bool,
    status: EvalStatus,
    score: f64,
    grader: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct TaskResult {
    id: String,
    line: usize,
    task: String,
    status: EvalStatus,
    score: Option<f64>,
    grader: &'static str,
    metrics: TaskMetrics,
    permissions_used: Vec<String>,
    checks: Vec<CheckResult>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CheckResult {
    id: String,
    status: EvalStatus,
    message: String,
    details: Value,
}

fn eval_agents(args: &SkillEvalOfflineArgs) -> std::result::Result<Vec<String>, CommandFailure> {
    if args.agent.is_some() && !args.matrix.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--agent and --matrix are mutually exclusive",
        ));
    }
    let raw_agents = if !args.matrix.is_empty() {
        args.matrix.clone()
    } else {
        vec![args.agent.clone().unwrap_or_else(|| "local".to_string())]
    };
    let mut agents = Vec::new();
    for raw in raw_agents {
        let agent = raw.trim();
        if agent.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "agent matrix entries must not be empty",
            ));
        }
        if !agent
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("agent id '{agent}' must match [a-z0-9_-]+"),
            ));
        }
        if !agents.iter().any(|existing| existing == agent) {
            agents.push(agent.to_string());
        }
    }
    Ok(agents)
}

fn read_jsonl<T: DeserializeOwned>(
    path: &Path,
) -> std::result::Result<Vec<JsonlRecord<T>>, CommandFailure> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(CommandFailure::new(
                ErrorCode::IoError,
                format!("failed to read eval file '{}': {err}", path.display()),
            ));
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
            schema_failure(
                format!(
                    "invalid eval JSON in '{}' at line {}: {err}",
                    path.display(),
                    line_no
                ),
                path,
                line_no,
            )
        })?;
        records.push(JsonlRecord {
            line: line_no,
            value,
        });
    }
    Ok(records)
}

fn evaluate_run(
    skill: &str,
    skill_path: &Path,
    agent: &str,
    model: Option<&str>,
    triggers: &[JsonlRecord<TriggerCase>],
    tasks: &[JsonlRecord<TaskCase>],
) -> std::result::Result<EvalRun, CommandFailure> {
    let trigger_results = triggers
        .iter()
        .enumerate()
        .map(|(index, record)| evaluate_trigger(skill, index, record))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let task_results = tasks
        .iter()
        .enumerate()
        .map(|(index, record)| evaluate_task(skill_path, index, record))
        .collect::<Vec<_>>();
    let summary = summarize_cases(&trigger_results, &task_results);

    Ok(EvalRun {
        agent: agent.to_string(),
        model: model.map(str::to_string),
        mode: "offline_fixture",
        summary,
        triggers: trigger_results,
        tasks: task_results,
    })
}

fn evaluate_trigger(
    skill: &str,
    index: usize,
    record: &JsonlRecord<TriggerCase>,
) -> std::result::Result<TriggerResult, CommandFailure> {
    let case = &record.value;
    let prompt = case.input.as_ref().ok_or_else(|| {
        schema_failure(
            "trigger eval case requires an input, prompt, or text field",
            Path::new("evals/triggers.jsonl"),
            record.line,
        )
    })?;
    let expected = expected_trigger(case).ok_or_else(|| {
        schema_failure(
            "trigger eval case requires expected_trigger, should_trigger, expected, or label",
            Path::new("evals/triggers.jsonl"),
            record.line,
        )
    })?;
    let observed = case
        .observed_trigger
        .or(case.actual_trigger)
        .unwrap_or_else(|| infer_trigger(skill, prompt));
    let passed = observed == expected;

    Ok(TriggerResult {
        id: case
            .id
            .clone()
            .unwrap_or_else(|| format!("trigger-{}", index + 1)),
        line: record.line,
        prompt: prompt.clone(),
        expected_trigger: expected,
        observed_trigger: observed,
        status: if passed {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        },
        score: if passed { 1.0 } else { 0.0 },
        grader: "deterministic_trigger_match",
    })
}

fn expected_trigger(case: &TriggerCase) -> Option<bool> {
    case.expected_trigger
        .or(case.should_trigger)
        .or_else(|| trigger_label(case.expected.as_deref()))
        .or_else(|| trigger_label(case.label.as_deref()))
}

fn trigger_label(label: Option<&str>) -> Option<bool> {
    match label?.trim().to_ascii_lowercase().as_str() {
        "positive" | "trigger" | "triggers" | "true" | "should_trigger" => Some(true),
        "negative" | "ignore" | "ignored" | "false" | "should_not_trigger" => Some(false),
        _ => None,
    }
}

fn infer_trigger(skill: &str, prompt: &str) -> bool {
    let prompt = prompt.to_ascii_lowercase();
    if prompt.contains(&skill.to_ascii_lowercase()) {
        return true;
    }
    skill
        .split(['-', '_', ' '])
        .filter(|part| part.len() >= 3)
        .any(|part| prompt.contains(&part.to_ascii_lowercase()))
}

fn evaluate_task(skill_path: &Path, index: usize, record: &JsonlRecord<TaskCase>) -> TaskResult {
    let case = &record.value;
    let mut checks = vec![
        contains_check(
            "outcome",
            &case.checks.outcome_contains,
            &case.output,
            "output contains expected outcome markers",
        ),
        contains_check(
            "process",
            &case.checks.process_contains,
            &case.trace.join("\n"),
            "trace contains expected process markers",
        ),
        contains_check(
            "style",
            &case.checks.style_contains,
            &case.output,
            "output contains expected style markers",
        ),
        efficiency_check(&case.metrics, &case.checks),
        artifact_check(skill_path, &case.checks.artifacts),
    ];
    let active = checks
        .iter()
        .filter(|check| check.status != EvalStatus::Skipped)
        .count();
    let passed = checks
        .iter()
        .filter(|check| check.status == EvalStatus::Passed)
        .count();
    let status = if active == 0 {
        EvalStatus::Skipped
    } else if active == passed {
        EvalStatus::Passed
    } else {
        EvalStatus::Failed
    };
    let score = if active == 0 {
        None
    } else {
        Some(passed as f64 / active as f64)
    };

    checks.sort_by(|a, b| a.id.cmp(&b.id));

    TaskResult {
        id: case
            .id
            .clone()
            .unwrap_or_else(|| format!("task-{}", index + 1)),
        line: record.line,
        task: case.task.clone().unwrap_or_default(),
        status,
        score,
        grader: "deterministic_local_checks",
        metrics: case.metrics.clone(),
        permissions_used: case.permissions_used.clone(),
        checks,
    }
}

fn contains_check(id: &str, needles: &[String], haystack: &str, message: &str) -> CheckResult {
    if needles.is_empty() {
        return CheckResult {
            id: id.to_string(),
            status: EvalStatus::Skipped,
            message: format!("{message}; no expectations declared"),
            details: json!({ "expected": [] }),
        };
    }
    let missing = needles
        .iter()
        .filter(|needle| !haystack.contains(needle.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    CheckResult {
        id: id.to_string(),
        status: if missing.is_empty() {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        },
        message: message.to_string(),
        details: json!({
            "expected": needles,
            "missing": missing,
        }),
    }
}

fn efficiency_check(metrics: &TaskMetrics, checks: &TaskChecks) -> CheckResult {
    if checks.max_tokens.is_none() && checks.max_commands.is_none() {
        return CheckResult {
            id: "efficiency".to_string(),
            status: EvalStatus::Skipped,
            message: "no efficiency limits declared".to_string(),
            details: json!({}),
        };
    }

    let token_ok = checks
        .max_tokens
        .is_none_or(|max| metrics.tokens.is_some_and(|actual| actual <= max));
    let command_ok = checks
        .max_commands
        .is_none_or(|max| metrics.commands.is_some_and(|actual| actual <= max));
    CheckResult {
        id: "efficiency".to_string(),
        status: if token_ok && command_ok {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        },
        message: "metrics stay within declared efficiency limits".to_string(),
        details: json!({
            "tokens": metrics.tokens,
            "commands": metrics.commands,
            "max_tokens": checks.max_tokens,
            "max_commands": checks.max_commands,
        }),
    }
}

fn artifact_check(skill_path: &Path, artifacts: &[ArtifactCheck]) -> CheckResult {
    if artifacts.is_empty() {
        return CheckResult {
            id: "artifacts".to_string(),
            status: EvalStatus::Skipped,
            message: "no artifact checks declared".to_string(),
            details: json!({ "artifacts": [] }),
        };
    }
    let results = artifacts
        .iter()
        .map(|artifact| one_artifact_check(skill_path, artifact))
        .collect::<Vec<_>>();
    let passed = results
        .iter()
        .all(|result| result["status"].as_str() == Some("passed"));
    CheckResult {
        id: "artifacts".to_string(),
        status: if passed {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        },
        message: "declared artifacts exist and match optional digests".to_string(),
        details: json!({ "artifacts": results }),
    }
}

fn one_artifact_check(skill_path: &Path, artifact: &ArtifactCheck) -> Value {
    let Some(path) = safe_artifact_path(skill_path, &artifact.path) else {
        return json!({
            "path": artifact.path,
            "status": "failed",
            "reason": "artifact path must be relative and stay within the skill directory",
        });
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            return json!({
                "path": artifact.path,
                "status": "failed",
                "reason": format!("failed to read artifact: {err}"),
            });
        }
    };
    let actual = sha256_hex(&bytes);
    let digest_ok = artifact
        .sha256
        .as_ref()
        .is_none_or(|expected| expected.eq_ignore_ascii_case(&actual));
    json!({
        "path": artifact.path,
        "status": if digest_ok { "passed" } else { "failed" },
        "sha256": actual,
        "expected_sha256": artifact.sha256,
    })
}

fn safe_artifact_path(skill_path: &Path, raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    if raw.trim().is_empty() || path.is_absolute() {
        return None;
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(skill_path.join(path))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

fn summarize_cases(triggers: &[TriggerResult], tasks: &[TaskResult]) -> EvalSummary {
    let mut summary = EvalSummary {
        case_count: triggers.len() + tasks.len(),
        ..EvalSummary::default()
    };
    let mut scores = Vec::new();
    let mut true_positive = 0usize;
    let mut false_positive = 0usize;
    let mut false_negative = 0usize;
    let mut task_passed = 0usize;
    let mut task_failed = 0usize;
    let mut permissions = BTreeSet::new();

    for trigger in triggers {
        count_status(&mut summary, trigger.status);
        scores.push(trigger.score);
        match (trigger.expected_trigger, trigger.observed_trigger) {
            (true, true) => true_positive += 1,
            (false, true) => false_positive += 1,
            (true, false) => false_negative += 1,
            (false, false) => {}
        }
    }
    for task in tasks {
        count_status(&mut summary, task.status);
        if let Some(score) = task.score {
            scores.push(score);
        }
        match task.status {
            EvalStatus::Passed => task_passed += 1,
            EvalStatus::Failed => task_failed += 1,
            EvalStatus::Skipped => {}
        }
        summary.token_count += task.metrics.tokens.unwrap_or(0);
        summary.command_count += task.metrics.commands.unwrap_or(0);
        permissions.extend(task.permissions_used.iter().cloned());
    }

    summary.aggregate_score = mean(&scores);
    summary.trigger_precision = ratio(true_positive, true_positive + false_positive);
    summary.trigger_recall = ratio(true_positive, true_positive + false_negative);
    summary.task_success_rate = ratio(task_passed, task_passed + task_failed);
    summary.permissions_used = permissions.into_iter().collect();
    summary
}

fn summarize_runs(runs: &[EvalRun]) -> EvalSummary {
    let mut summary = EvalSummary::default();
    let mut scores = Vec::new();
    let mut precision_values = Vec::new();
    let mut recall_values = Vec::new();
    let mut task_rate_values = Vec::new();
    let mut permissions = BTreeSet::new();

    for run in runs {
        summary.case_count += run.summary.case_count;
        summary.passed += run.summary.passed;
        summary.failed += run.summary.failed;
        summary.skipped += run.summary.skipped;
        summary.token_count += run.summary.token_count;
        summary.command_count += run.summary.command_count;
        if let Some(score) = run.summary.aggregate_score {
            scores.push(score);
        }
        if let Some(value) = run.summary.trigger_precision {
            precision_values.push(value);
        }
        if let Some(value) = run.summary.trigger_recall {
            recall_values.push(value);
        }
        if let Some(value) = run.summary.task_success_rate {
            task_rate_values.push(value);
        }
        permissions.extend(run.summary.permissions_used.iter().cloned());
    }

    summary.aggregate_score = mean(&scores);
    summary.trigger_precision = mean(&precision_values);
    summary.trigger_recall = mean(&recall_values);
    summary.task_success_rate = mean(&task_rate_values);
    summary.permissions_used = permissions.into_iter().collect();
    summary
}

fn count_status(summary: &mut EvalSummary, status: EvalStatus) {
    match status {
        EvalStatus::Passed => summary.passed += 1,
        EvalStatus::Failed => summary.failed += 1,
        EvalStatus::Skipped => summary.skipped += 1,
    }
}

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn schema_failure(message: impl Into<String>, path: &Path, line: usize) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::SchemaMismatch, message);
    failure.details = json!({
        "path": path.display().to_string(),
        "line": line,
    });
    failure
}
