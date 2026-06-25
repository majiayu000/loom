use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::SkillEvalArgs;
use crate::envelope::Meta;
use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

use super::helpers::{map_arg, validate_skill_name};
use super::skill_verify::{head_tree_oid_for_path, last_commit_for_path};
use super::{App, CommandFailure};

const EVAL_SCHEMA_VERSION: u32 = 1;

impl App {
    pub fn cmd_skill_eval(
        &self,
        args: &SkillEvalArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let skill_path = self.ctx.skill_path(&args.skill);
        if !skill_path.is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let agents = eval_agents(args)?;
        let evals_dir = skill_path.join("evals");
        let trigger_path = evals_dir.join("triggers.jsonl");
        let task_path = evals_dir.join("tasks.jsonl");
        let triggers = read_jsonl::<TriggerCase>(&trigger_path)?;
        let tasks = read_jsonl::<TaskCase>(&task_path)?;

        let skill_rel = format!("skills/{}", args.skill);
        let version = SkillEvalVersion {
            head_tree_oid: head_tree_oid_for_path(&self.ctx, &skill_rel).ok().flatten(),
            last_source_commit: last_commit_for_path(&self.ctx, &skill_rel).ok().flatten(),
        };

        let runs = agents
            .iter()
            .map(|agent| {
                evaluate_run(
                    &args.skill,
                    &skill_path,
                    agent,
                    args.model.as_deref(),
                    &triggers,
                    &tasks,
                )
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let summary = summarize_runs(&runs);
        let failed = summary.failed;

        let warnings = if triggers.is_empty() && tasks.is_empty() {
            vec![format!(
                "no eval cases found under {}; expected triggers.jsonl and/or tasks.jsonl",
                evals_dir.display()
            )]
        } else {
            Vec::new()
        };

        let report = json!({
            "schema_version": EVAL_SCHEMA_VERSION,
            "skill": args.skill,
            "skill_version": version,
            "eval_root": evals_dir.display().to_string(),
            "matrix": agents,
            "summary": summary,
            "runs": runs,
            "security_model": {
                "eval_success_is_safety_guarantee": false,
                "note": "Eval success is quality evidence only. It does not prove the skill is safe, sandboxed, or free of prompt-injection risk."
            }
        });
        if failed > 0 {
            let mut failure = CommandFailure::new(
                ErrorCode::EvalFailed,
                format!("skill eval failed with {failed} failing case(s)"),
            );
            failure.details = json!({
                "failed": failed,
                "report": report,
            });
            return Err(failure);
        }

        Ok((
            report,
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
struct SkillEvalVersion {
    head_tree_oid: Option<String>,
    last_source_commit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TriggerCase {
    #[serde(default)]
    id: Option<String>,
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
struct TaskCase {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, alias = "input")]
    task: Option<String>,
    #[serde(default)]
    output: String,
    #[serde(default)]
    trace: Vec<String>,
    #[serde(default)]
    metrics: TaskMetrics,
    #[serde(default)]
    permissions_used: Vec<String>,
    #[serde(default)]
    checks: TaskChecks,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
struct TaskMetrics {
    tokens: Option<u64>,
    commands: Option<u64>,
    duration_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct TaskChecks {
    #[serde(default)]
    outcome_contains: Vec<String>,
    #[serde(default)]
    process_contains: Vec<String>,
    #[serde(default)]
    style_contains: Vec<String>,
    #[serde(default)]
    max_tokens: Option<u64>,
    #[serde(default)]
    max_commands: Option<u64>,
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
struct JsonlRecord<T> {
    line: usize,
    value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvalStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Serialize)]
struct EvalRun {
    agent: String,
    model: Option<String>,
    mode: &'static str,
    summary: EvalSummary,
    triggers: Vec<TriggerResult>,
    tasks: Vec<TaskResult>,
}

#[derive(Debug, Default, Clone, Serialize)]
struct EvalSummary {
    case_count: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    aggregate_score: Option<f64>,
    trigger_precision: Option<f64>,
    trigger_recall: Option<f64>,
    task_success_rate: Option<f64>,
    token_count: u64,
    command_count: u64,
    permissions_used: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TriggerResult {
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
struct TaskResult {
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
struct CheckResult {
    id: String,
    status: EvalStatus,
    message: String,
    details: Value,
}

fn eval_agents(args: &SkillEvalArgs) -> std::result::Result<Vec<String>, CommandFailure> {
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
