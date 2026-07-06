use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{EvalBaselineArg, EvalRunnerArg};
use crate::commands::{CommandFailure, redact_sensitive_string};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::cases::{
    HarnessJsonlRecord, HarnessTaskCase, HarnessTriggerCase, HarnessTriggerResult,
    evaluate_trigger_case, trigger_precision, trigger_recall,
};
use super::report::{
    default_report_path, harness_ratio, io_failure, option_delta, runner_id, security_model,
};
use crate::commands::file_ops::copy_dir_recursive_without_symlinks;

pub(super) trait SkillEvalRunner {
    fn prepare(
        &mut self,
        plan: &EvalPlan,
    ) -> std::result::Result<EvalRunEnvironment, CommandFailure>;
    fn run_case(
        &mut self,
        env: &EvalRunEnvironment,
        plan: &EvalPlan,
        case: &HarnessTaskCase,
        case_key: &str,
        variant: EvalVariant,
        attempt: u32,
    ) -> std::result::Result<EvalCaseResult, CommandFailure>;
    fn run_trigger_case(
        &mut self,
        _env: &EvalRunEnvironment,
        plan: &EvalPlan,
        attempt: u32,
        record: &HarnessJsonlRecord<HarnessTriggerCase>,
    ) -> std::result::Result<HarnessTriggerResult, CommandFailure> {
        evaluate_trigger_case(&plan.skill, &plan.agent, attempt, record)
    }
    fn cleanup(&mut self, env: EvalRunEnvironment) -> CleanupResult;
}

pub(super) struct MockAgentRunner;

impl SkillEvalRunner for MockAgentRunner {
    fn prepare(
        &mut self,
        _plan: &EvalPlan,
    ) -> std::result::Result<EvalRunEnvironment, CommandFailure> {
        let path = std::env::temp_dir().join(format!("loom-eval-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|err| io_failure("eval_temp_prepare", &path, err))?;
        Ok(EvalRunEnvironment { root: path })
    }

    fn run_case(
        &mut self,
        env: &EvalRunEnvironment,
        plan: &EvalPlan,
        case: &HarnessTaskCase,
        case_key: &str,
        variant: EvalVariant,
        attempt: u32,
    ) -> std::result::Result<EvalCaseResult, CommandFailure> {
        let workspace = isolated_workspace(env, case_key, variant, attempt);
        prepare_workspace(plan, case, &workspace)?;
        Ok(mock_case_result(plan, case, variant, attempt, workspace))
    }

    fn cleanup(&mut self, env: EvalRunEnvironment) -> CleanupResult {
        if std::env::var("LOOM_EVAL_MOCK_CLEANUP_FAIL").ok().as_deref() == Some("1") {
            let _ = fs::remove_dir_all(&env.root);
            return CleanupResult::failure("cleanup failure injected for eval test");
        }
        match fs::remove_dir_all(&env.root) {
            Ok(()) => CleanupResult::passed("temporary eval workspaces removed"),
            Err(err) => CleanupResult::failure(&format!(
                "failed to remove temporary eval workspaces: {err}"
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct EvalPlan {
    pub(super) skill: String,
    pub(super) agent: String,
    pub(super) mode: &'static str,
    pub(super) runner: EvalRunnerArg,
    pub(super) baseline: Option<EvalBaselineArg>,
    pub(super) runs: u32,
    pub(super) workspace: Option<PathBuf>,
    pub(super) cases_path: PathBuf,
    pub(super) output_path: Option<PathBuf>,
    pub(super) report_path: PathBuf,
    pub(super) skill_source: Option<String>,
}

pub(super) struct EvalPlanInput {
    pub(super) skill: String,
    pub(super) agent: String,
    pub(super) runner: EvalRunnerArg,
    pub(super) baseline: EvalBaselineArg,
    pub(super) runs: u32,
    pub(super) workspace: Option<PathBuf>,
    pub(super) cases_path: PathBuf,
    pub(super) output_path: Option<PathBuf>,
    pub(super) skill_source: Option<String>,
}

pub(super) struct EvalTriggerPlanInput {
    pub(super) skill: String,
    pub(super) agent: String,
    pub(super) runner: EvalRunnerArg,
    pub(super) runs: u32,
    pub(super) cases_path: PathBuf,
    pub(super) output_path: Option<PathBuf>,
    pub(super) skill_source: Option<String>,
}

impl EvalPlan {
    pub(super) fn run(ctx: &AppContext, input: EvalPlanInput) -> Self {
        let skill = input.skill;
        Self {
            report_path: default_report_path(ctx, &skill, "run"),
            skill,
            agent: input.agent,
            mode: mode_for_runner(input.runner, "real_agent_baseline"),
            runner: input.runner,
            baseline: Some(input.baseline),
            runs: input.runs,
            workspace: input.workspace,
            cases_path: input.cases_path,
            output_path: input.output_path,
            skill_source: input.skill_source,
        }
    }

    pub(super) fn trigger(ctx: &AppContext, input: EvalTriggerPlanInput) -> Self {
        let skill = input.skill;
        Self {
            report_path: default_report_path(ctx, &skill, "trigger"),
            skill,
            agent: input.agent,
            mode: mode_for_runner(input.runner, "trigger_quality"),
            runner: input.runner,
            baseline: None,
            runs: input.runs,
            workspace: None,
            cases_path: input.cases_path,
            output_path: input.output_path,
            skill_source: input.skill_source,
        }
    }

    pub(super) fn to_value(&self, case_views: &[Value], will_write_report: bool) -> Value {
        json!({
            "skill": self.skill,
            "agent": self.agent,
            "mode": self.mode,
            "runner": runner_id(self.runner),
            "baseline": self.baseline.map(baseline_id),
            "runs": self.runs,
            "workspace": self.workspace.as_ref().map(|path| path.display().to_string()),
            "cases_path": self.cases_path.display().to_string(),
            "output_path": self.output_path.as_ref().map(|path| path.display().to_string()),
            "default_report_path": self.report_path.display().to_string(),
            "skill_source_included": self.skill_source.is_some(),
            "will_write_report": will_write_report,
            "resolved_cases": case_views,
            "actions": [
                "load cases",
                "prepare isolated temp workspaces",
                "run with-skill task cases",
                "run no-skill baseline task cases",
                "persist report outside the skill source"
            ]
        })
    }
}

#[derive(Debug)]
pub(super) struct EvalRunEnvironment {
    pub(super) root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EvalVariant {
    WithSkill,
    WithoutSkill,
}

#[derive(Debug, Serialize)]
pub(super) struct EvalCaseResult {
    pub(super) id: String,
    pub(super) attempt: u32,
    pub(super) variant: EvalVariant,
    pub(super) status: &'static str,
    pub(super) score: Option<f64>,
    pub(super) output: String,
    pub(super) exit_code: i32,
    pub(super) commands: Vec<String>,
    pub(super) files_changed: Vec<String>,
    pub(super) metrics: Metrics,
    pub(super) checks: Vec<MockCheckResult>,
    pub(super) workspace: String,
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub(super) struct Metrics {
    pub(super) tokens: Option<u64>,
    pub(super) commands: Option<u64>,
    pub(super) duration_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(super) struct MockCheckResult {
    pub(super) id: &'static str,
    pub(super) status: &'static str,
    pub(super) message: String,
    pub(super) details: Value,
}

pub(super) struct CleanupResult {
    failed: bool,
    pub(super) message: String,
}

impl CleanupResult {
    pub(super) fn passed(message: &str) -> Self {
        Self {
            failed: false,
            message: message.to_string(),
        }
    }

    pub(super) fn failure(message: &str) -> Self {
        Self {
            failed: true,
            message: message.to_string(),
        }
    }

    pub(super) fn failed(&self) -> bool {
        self.failed
    }
}

pub(super) fn run_task_baseline(
    runner: &mut dyn SkillEvalRunner,
    env: &EvalRunEnvironment,
    plan: &EvalPlan,
    cases: &[HarnessJsonlRecord<HarnessTaskCase>],
    triggers: &[HarnessJsonlRecord<HarnessTriggerCase>],
) -> std::result::Result<Value, CommandFailure> {
    let runs = super::report::normalize_runs(plan.runs)?;
    let mut with_skill = Vec::new();
    let mut without_skill = Vec::new();
    for attempt in 1..=runs {
        for record in cases {
            let case_key = case_workspace_key(record.line, &record.value);
            with_skill.push(runner.run_case(
                env,
                plan,
                &record.value,
                &case_key,
                EvalVariant::WithSkill,
                attempt,
            )?);
            without_skill.push(runner.run_case(
                env,
                plan,
                &record.value,
                &case_key,
                EvalVariant::WithoutSkill,
                attempt,
            )?);
        }
    }
    let mut trigger_results = Vec::new();
    for record in triggers {
        trigger_results.push(runner.run_trigger_case(env, plan, 1, record)?);
    }
    let summary = summarize_baseline(&with_skill, &without_skill, &trigger_results, cases.len());
    Ok(json!({
        "schema_version": super::EVAL_SCHEMA_VERSION,
        "skill": plan.skill,
        "agent": plan.agent,
        "mode": plan.mode,
        "runner": runner_id(plan.runner),
        "baseline": plan.baseline.map(baseline_id),
        "plan": plan.to_value(&[], true),
        "summary": summary,
        "skill_version": {"head_tree_oid": null, "last_source_commit": null},
        "runs": {"with_skill": with_skill, "without_skill": without_skill, "triggers": trigger_results},
        "decision": activation_decision(&summary),
        "security_model": security_model(),
    }))
}

pub(super) fn summarize_mock_version(
    skill: &str,
    cases: &[HarnessJsonlRecord<HarnessTaskCase>],
    side: &str,
) -> VersionSummary {
    let plan = EvalPlan {
        skill: skill.to_string(),
        agent: "mock".to_string(),
        mode: "version_compare",
        runner: EvalRunnerArg::Mock,
        baseline: None,
        runs: 1,
        workspace: None,
        cases_path: PathBuf::new(),
        output_path: None,
        report_path: PathBuf::new(),
        skill_source: None,
    };
    let results = cases
        .iter()
        .map(|record| {
            mock_case_result(
                &plan,
                &record.value,
                EvalVariant::WithSkill,
                1,
                PathBuf::from(format!("<isolated-{side}>")),
            )
        })
        .collect::<Vec<_>>();
    VersionSummary {
        case_count: cases.len(),
        pass_rate: pass_rate(&results),
        failed: results
            .iter()
            .filter(|result| result.status == "failed")
            .count(),
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct VersionSummary {
    pub(super) case_count: usize,
    pub(super) pass_rate: Option<f64>,
    pub(super) failed: usize,
}

fn mock_case_result(
    plan: &EvalPlan,
    case: &HarnessTaskCase,
    variant: EvalVariant,
    attempt: u32,
    workspace: PathBuf,
) -> EvalCaseResult {
    let commands = match variant {
        EvalVariant::WithSkill if !case.checks.commands_contains.is_empty() => {
            case.checks.commands_contains.clone()
        }
        EvalVariant::WithSkill => vec!["loom skill eval".to_string()],
        EvalVariant::WithoutSkill => Vec::new(),
    };
    let files_changed = if variant == EvalVariant::WithSkill {
        case.checks.files_changed.clone()
    } else {
        Vec::new()
    };
    let output = if variant == EvalVariant::WithSkill {
        format!("mock {} result: tests pass; task complete", plan.skill)
    } else {
        "mock baseline without skill: incomplete".to_string()
    };
    let exit_code = match (variant, case.checks.exit_code) {
        (EvalVariant::WithSkill, Some(expected)) => expected,
        (EvalVariant::WithSkill, None) => 0,
        (EvalVariant::WithoutSkill, Some(0)) => 1,
        (EvalVariant::WithoutSkill, Some(expected)) => expected,
        (EvalVariant::WithoutSkill, None) => 0,
    };
    let metrics = Metrics {
        tokens: Some(if variant == EvalVariant::WithSkill {
            120
        } else {
            100
        }),
        commands: Some(commands.len() as u64),
        duration_ms: Some(if variant == EvalVariant::WithSkill {
            1200
        } else {
            1000
        }),
    };
    let checks = grade_eval_result(case, &output, exit_code, &commands, &files_changed, metrics);
    let (status, score) = case_status_score(&checks);
    EvalCaseResult {
        id: case.id(),
        attempt,
        variant,
        status,
        score,
        output: redact_sensitive_string(&output),
        exit_code,
        commands,
        files_changed,
        metrics,
        checks,
        workspace: workspace.display().to_string(),
    }
}

pub(super) fn grade_eval_result(
    case: &HarnessTaskCase,
    output: &str,
    exit_code: i32,
    commands: &[String],
    files_changed: &[String],
    metrics: Metrics,
) -> Vec<MockCheckResult> {
    let command_text = commands.join("\n");
    let mut checks = vec![
        mock_contains_check("outcome", &case.checks.outcome_contains, output),
        mock_contains_check("commands", &case.checks.commands_contains, &command_text),
        list_contains_check("files_changed", &case.checks.files_changed, files_changed),
        exit_code_check(case.checks.exit_code, exit_code),
        max_check("max_tokens", case.checks.max_tokens, metrics.tokens),
        max_check("max_commands", case.checks.max_commands, metrics.commands),
    ];
    checks.sort_by(|left, right| left.id.cmp(right.id));
    checks
}

pub(super) fn case_status_score(checks: &[MockCheckResult]) -> (&'static str, Option<f64>) {
    let active = checks
        .iter()
        .filter(|check| check.status != "skipped")
        .count();
    let passed = checks
        .iter()
        .filter(|check| check.status == "passed")
        .count();
    let status = if active == 0 {
        "skipped"
    } else if active == passed {
        "passed"
    } else {
        "failed"
    };
    let score = (active != 0).then_some(passed as f64 / active as f64);
    (status, score)
}

fn mock_contains_check(id: &'static str, needles: &[String], haystack: &str) -> MockCheckResult {
    if needles.is_empty() {
        return skipped_check(id, "no expectation declared");
    }
    let missing = needles
        .iter()
        .filter(|needle| !haystack.contains(needle.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    MockCheckResult {
        id,
        status: if missing.is_empty() {
            "passed"
        } else {
            "failed"
        },
        message: "text contains expected markers".to_string(),
        details: json!({"expected": needles, "missing": missing}),
    }
}

fn list_contains_check(
    id: &'static str,
    expected: &[String],
    actual: &[String],
) -> MockCheckResult {
    if expected.is_empty() {
        return skipped_check(id, "no expectation declared");
    }
    let missing = expected
        .iter()
        .filter(|item| !actual.iter().any(|actual| actual == *item))
        .cloned()
        .collect::<Vec<_>>();
    MockCheckResult {
        id,
        status: if missing.is_empty() {
            "passed"
        } else {
            "failed"
        },
        message: "list contains expected entries".to_string(),
        details: json!({"expected": expected, "actual": actual, "missing": missing}),
    }
}

fn exit_code_check(expected: Option<i32>, actual: i32) -> MockCheckResult {
    let Some(expected) = expected else {
        return skipped_check("exit_code", "no expectation declared");
    };
    MockCheckResult {
        id: "exit_code",
        status: if expected == actual {
            "passed"
        } else {
            "failed"
        },
        message: "exit code matches expectation".to_string(),
        details: json!({"expected": expected, "actual": actual}),
    }
}

fn max_check(id: &'static str, max: Option<u64>, actual: Option<u64>) -> MockCheckResult {
    let Some(max) = max else {
        return skipped_check(id, "no limit declared");
    };
    MockCheckResult {
        id,
        status: if actual.is_some_and(|actual| actual <= max) {
            "passed"
        } else {
            "failed"
        },
        message: "metric stays within declared limit".to_string(),
        details: json!({"max": max, "actual": actual}),
    }
}

fn skipped_check(id: &'static str, message: &str) -> MockCheckResult {
    MockCheckResult {
        id,
        status: "skipped",
        message: message.to_string(),
        details: json!({}),
    }
}

fn summarize_baseline(
    with_skill: &[EvalCaseResult],
    without_skill: &[EvalCaseResult],
    triggers: &[HarnessTriggerResult],
    logical_case_count: usize,
) -> Value {
    let with_rate = pass_rate(with_skill);
    let without_rate = pass_rate(without_skill);
    let failed = with_skill
        .iter()
        .filter(|case| case.status == "failed")
        .count()
        + triggers
            .iter()
            .filter(|case| case.status == "failed")
            .count();
    json!({
        "case_count": logical_case_count,
        "with_skill_pass_rate": with_rate,
        "without_skill_pass_rate": without_rate,
        "delta": option_delta(with_rate, without_rate),
        "trigger_precision": trigger_precision(triggers),
        "trigger_recall": trigger_recall(triggers),
        "token_overhead_ratio": overhead_ratio(with_skill, without_skill, |metrics| metrics.tokens),
        "command_overhead_ratio": overhead_ratio(with_skill, without_skill, |metrics| metrics.commands),
        "duration_overhead_ratio": overhead_ratio(with_skill, without_skill, |metrics| metrics.duration_ms),
        "failed": failed,
    })
}

fn activation_decision(summary: &Value) -> Value {
    let delta = summary["delta"].as_f64();
    let failed = summary["failed"].as_u64().unwrap_or(0);
    let recommend = delta.is_some_and(|delta| delta > 0.0) && failed == 0;
    json!({
        "recommend_activation": recommend,
        "reason": if recommend {
            "positive pass-rate delta with no failing eval cases"
        } else {
            "no positive clean pass-rate delta"
        }
    })
}

fn pass_rate(results: &[EvalCaseResult]) -> Option<f64> {
    let passed = results
        .iter()
        .filter(|case| case.status == "passed")
        .count();
    let failed = results
        .iter()
        .filter(|case| case.status == "failed")
        .count();
    harness_ratio(passed, passed + failed)
}

fn overhead_ratio(
    with_skill: &[EvalCaseResult],
    without_skill: &[EvalCaseResult],
    metric: fn(Metrics) -> Option<u64>,
) -> Option<f64> {
    let with_total = metric_total(with_skill, metric)?;
    let without_total = metric_total(without_skill, metric)?;
    (without_total != 0)
        .then_some((with_total as f64 - without_total as f64) / without_total as f64)
}

fn metric_total(results: &[EvalCaseResult], metric: fn(Metrics) -> Option<u64>) -> Option<u64> {
    let mut total = 0u64;
    let mut seen = false;
    for result in results {
        if let Some(value) = metric(result.metrics) {
            total += value;
            seen = true;
        }
    }
    seen.then_some(total)
}

pub(super) fn prepare_workspace(
    plan: &EvalPlan,
    case: &HarnessTaskCase,
    workspace: &Path,
) -> std::result::Result<(), CommandFailure> {
    if let Some(parent) = workspace.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| io_failure("eval_workspace_parent", parent, err))?;
    }
    let source = case
        .workspace_fixture
        .as_deref()
        .and_then(|fixture| safe_fixture_path(&plan.cases_path, fixture))
        .or_else(|| plan.workspace.clone());
    match source {
        Some(source) => copy_dir_recursive_without_symlinks(&source, workspace)
            .map_err(|err| CommandFailure::new(ErrorCode::EvalFailed, err.to_string())),
        None => fs::create_dir_all(workspace)
            .map_err(|err| io_failure("eval_workspace_create", workspace, err)),
    }
}

fn safe_fixture_path(cases_path: &Path, raw: &str) -> Option<PathBuf> {
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
    cases_path.parent().map(|parent| parent.join(path))
}

pub(super) fn isolated_workspace(
    env: &EvalRunEnvironment,
    case_key: &str,
    variant: EvalVariant,
    attempt: u32,
) -> PathBuf {
    let variant = match variant {
        EvalVariant::WithSkill => "with-skill",
        EvalVariant::WithoutSkill => "without-skill",
    };
    env.root.join(format!("{case_key}-{variant}-{attempt}"))
}

pub(super) fn case_workspace_key(line: usize, case: &HarnessTaskCase) -> String {
    format!("line{}-{}", line, slug_path_component(&case.id()))
}

pub(super) fn slug_path_component(raw: &str) -> String {
    let mut slug = String::new();
    let mut last_sep = false;
    for ch in raw.chars().take(80) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_sep = false;
        } else if matches!(ch, '-' | '_' | '.') {
            slug.push(ch);
            last_sep = false;
        } else if !last_sep {
            slug.push('_');
            last_sep = true;
        }
    }
    let slug = slug.trim_matches(['.', '_', '-']);
    if slug.is_empty() || slug == "." || slug == ".." {
        "case".to_string()
    } else {
        slug.to_string()
    }
}

fn baseline_id(baseline: EvalBaselineArg) -> &'static str {
    match baseline {
        EvalBaselineArg::NoSkill => "no-skill",
    }
}

fn mode_for_runner(runner: EvalRunnerArg, fallback: &'static str) -> &'static str {
    match runner {
        EvalRunnerArg::Mock => fallback,
        EvalRunnerArg::CodexCli => "real_codex_cli",
    }
}
