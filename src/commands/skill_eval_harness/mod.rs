pub(crate) mod cases;
mod report;
mod runner;

use std::path::Path;

use serde_json::json;

use crate::cli::{SkillEvalCompareArgs, SkillEvalRunArgs, SkillEvalTriggerArgs};
use crate::envelope::Meta;
use crate::gitops::run_git_allow_failure;
use crate::state::AppContext;

use super::{App, CommandFailure};
use cases::{
    HarnessTaskCase, HarnessTriggerCase, evaluate_trigger_case, read_harness_jsonl,
    read_harness_jsonl_str, read_required_harness_jsonl, summarize_trigger_results,
};
use report::{
    cleanup_to_value, ensure_runner_available, eval_failed, persist_report, require_skill,
    resolve_cases_path, resolve_compare_version, runner_id, security_model,
};
use runner::{
    EvalPlan, EvalPlanInput, MockAgentRunner, SkillEvalRunner, run_task_baseline,
    summarize_mock_version,
};

const EVAL_SCHEMA_VERSION: u32 = 1;

impl App {
    pub(crate) fn cmd_skill_eval_run(
        &self,
        args: &SkillEvalRunArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_path = require_skill(&self.ctx, &args.skill)?;
        let cases_path =
            resolve_cases_path(&skill_path, args.cases.as_deref(), "evals/tasks.jsonl");
        let triggers_path = skill_path.join("evals/triggers.jsonl");
        let cases = if args.cases.is_some() {
            read_required_harness_jsonl::<HarnessTaskCase>(&cases_path)?
        } else {
            read_harness_jsonl::<HarnessTaskCase>(&cases_path)?
        };
        let triggers = read_harness_jsonl::<HarnessTriggerCase>(&triggers_path)?;
        let plan = EvalPlan::run(
            &self.ctx,
            EvalPlanInput {
                skill: args.skill.clone(),
                agent: args.agent.clone(),
                runner: args.runner,
                baseline: args.baseline,
                runs: args.runs,
                workspace: args.workspace.clone(),
                cases_path,
                output_path: args.output.clone(),
            },
        );
        let case_views = cases
            .iter()
            .map(|record| json!({"id": record.value.id(), "line": record.line}))
            .collect::<Vec<_>>();

        if args.dry_run {
            return Ok((
                json!({
                    "schema_version": EVAL_SCHEMA_VERSION,
                    "skill": args.skill,
                    "agent": args.agent,
                    "mode": "real_agent_baseline",
                    "runner": runner_id(args.runner),
                    "dry_run": true,
                    "plan": plan.to_value(&case_views, false),
                    "skill_version": super::skill_eval::skill_eval_version(&self.ctx, &args.skill),
                    "security_model": security_model(),
                }),
                Meta::default(),
            ));
        }

        ensure_runner_available(args.runner)?;
        let mut runner = MockAgentRunner;
        let env = runner.prepare(&plan)?;
        let mut report = match run_task_baseline(&mut runner, &env, &plan, &cases, &triggers) {
            Ok(report) => report,
            Err(mut failure) => {
                let cleanup = runner.cleanup(env);
                failure.details["cleanup"] = cleanup_to_value(&cleanup);
                return Err(failure);
            }
        };
        let cleanup = runner.cleanup(env);
        report["skill_version"] = json!(super::skill_eval::skill_eval_version(
            &self.ctx,
            &args.skill
        ));
        report["cleanup"] = cleanup_to_value(&cleanup);
        persist_report(
            &self.ctx,
            &args.skill,
            "run",
            args.output.as_deref(),
            &mut report,
        )?;

        let failed = report["summary"]["failed"].as_u64().unwrap_or(0);
        if failed > 0 || cleanup.failed() {
            return Err(eval_failed(
                "skill eval run failed",
                failed,
                "eval_run_failed",
                report,
            ));
        }
        Ok((report, Meta::default()))
    }

    pub(crate) fn cmd_skill_eval_trigger(
        &self,
        args: &SkillEvalTriggerArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_path = require_skill(&self.ctx, &args.skill)?;
        ensure_runner_available(args.runner)?;
        let cases_path =
            resolve_cases_path(&skill_path, args.cases.as_deref(), "evals/triggers.jsonl");
        let triggers = if args.cases.is_some() {
            read_required_harness_jsonl::<HarnessTriggerCase>(&cases_path)?
        } else {
            read_harness_jsonl::<HarnessTriggerCase>(&cases_path)?
        };
        let runs = report::normalize_runs(args.runs)?;
        let mut results = Vec::new();
        for attempt in 1..=runs {
            for record in &triggers {
                results.push(evaluate_trigger_case(
                    &args.skill,
                    &args.agent,
                    attempt,
                    record,
                )?);
            }
        }
        let mut report = json!({
            "schema_version": EVAL_SCHEMA_VERSION,
            "skill": args.skill,
            "agent": args.agent,
            "mode": "trigger_quality",
            "runner": runner_id(args.runner),
            "cases_path": cases_path.display().to_string(),
            "runs": runs,
            "summary": summarize_trigger_results(&results),
            "skill_version": super::skill_eval::skill_eval_version(&self.ctx, &args.skill),
            "results": results,
            "security_model": security_model(),
        });
        persist_report(
            &self.ctx,
            &args.skill,
            "trigger",
            args.output.as_deref(),
            &mut report,
        )?;
        let failed = report["summary"]["failed"].as_u64().unwrap_or(0);
        if failed > 0 {
            return Err(eval_failed(
                "skill trigger eval failed",
                failed,
                "trigger_eval_failed",
                report,
            ));
        }
        Ok((report, Meta::default()))
    }

    pub(crate) fn cmd_skill_eval_compare(
        &self,
        args: &SkillEvalCompareArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_path = require_skill(&self.ctx, &args.skill)?;
        ensure_runner_available(args.runner)?;
        let cases_path =
            resolve_cases_path(&skill_path, args.cases.as_deref(), "evals/tasks.jsonl");
        let from_cases = compare_cases(
            &self.ctx,
            &args.skill,
            &args.from_ref,
            args.cases.as_deref(),
            &skill_path,
        )?;
        let to_cases = compare_cases(
            &self.ctx,
            &args.skill,
            &args.to_ref,
            args.cases.as_deref(),
            &skill_path,
        )?;
        let from_version = resolve_compare_version(&self.ctx, &args.skill, &args.from_ref)?;
        let to_version = resolve_compare_version(&self.ctx, &args.skill, &args.to_ref)?;
        let from_summary = summarize_mock_version(&args.skill, &from_cases, "from");
        let to_summary = summarize_mock_version(&args.skill, &to_cases, "to");
        let mut report = json!({
            "schema_version": EVAL_SCHEMA_VERSION,
            "skill": args.skill,
            "agent": args.agent,
            "mode": "version_compare",
            "runner": runner_id(args.runner),
            "cases_path": cases_path.display().to_string(),
            "from": {"ref": args.from_ref, "skill_version": from_version, "summary": from_summary},
            "to": {"ref": args.to_ref, "skill_version": to_version, "summary": to_summary},
            "summary": {
                "case_count": to_cases.len(),
                "from_case_count": from_cases.len(),
                "from_pass_rate": from_summary.pass_rate,
                "to_pass_rate": to_summary.pass_rate,
                "delta": report::option_delta(to_summary.pass_rate, from_summary.pass_rate),
                "trigger_precision": null,
                "trigger_recall": null,
                "token_overhead_ratio": null,
                "command_overhead_ratio": null,
            },
            "security_model": security_model(),
        });
        persist_report(
            &self.ctx,
            &args.skill,
            "compare",
            args.output.as_deref(),
            &mut report,
        )?;
        Ok((report, Meta::default()))
    }
}

fn compare_cases(
    ctx: &AppContext,
    skill: &str,
    reference: &str,
    explicit_cases: Option<&Path>,
    skill_path: &Path,
) -> std::result::Result<Vec<cases::HarnessJsonlRecord<HarnessTaskCase>>, CommandFailure> {
    if let Some(path) = explicit_cases {
        let path = resolve_cases_path(skill_path, Some(path), "evals/tasks.jsonl");
        return read_required_harness_jsonl::<HarnessTaskCase>(&path);
    }
    if reference == "working-tree" {
        let path = skill_path.join("evals/tasks.jsonl");
        return read_harness_jsonl::<HarnessTaskCase>(&path);
    }
    let spec = format!("{reference}:skills/{skill}/evals/tasks.jsonl");
    let output = run_git_allow_failure(ctx, &["show", &spec]).map_err(|err| {
        report::eval_failed(
            "compare cases could not be read",
            0,
            "compare_cases_read_failed",
            json!({"ref": reference, "error": err.to_string()}),
        )
    })?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    read_harness_jsonl_str::<HarnessTaskCase>(&String::from_utf8_lossy(&output.stdout), &spec)
}
