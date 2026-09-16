use std::fs;

use serde_json::{Value, json};

use crate::cli::{EvalBaselineArg, EvalRunnerArg, SkillsetEvalArgs, SkillsetEvalBaselineArg};
use crate::commands::CommandFailure;
use crate::commands::skill_safety::enforce_skill_safety;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::cases::{HarnessTaskCase, HarnessTriggerCase, read_harness_jsonl};
use super::eval_runner;
use super::report::{cleanup_to_value, ensure_runner_available, io_failure, runner_id};
use super::runner::{EvalPlan, EvalPlanInput, EvalVariant, case_workspace_key, pass_rate};

/// Reuse the single-skill runner and graders with the whole bundle as one input.
pub(crate) fn run_skillset_eval(
    ctx: &AppContext,
    args: &SkillsetEvalArgs,
    members: &[&str],
) -> Result<Value, CommandFailure> {
    let runner_kind = args
        .runner
        .ok_or_else(|| invalid("select an eval runner"))?;
    if members.is_empty() {
        return Err(invalid(
            "skillset eval requires at least one available member",
        ));
    }
    if runner_kind == EvalRunnerArg::CodexCli && args.agent != "codex" {
        return Err(invalid("codex-cli evaluates only --agent codex"));
    }
    let eval_root = ctx.root.join("skillsets").join(&args.name).join("evals");
    let tasks = read_harness_jsonl::<HarnessTaskCase>(&eval_root.join("tasks.jsonl"))?;
    let triggers = read_harness_jsonl::<HarnessTriggerCase>(&eval_root.join("triggers.jsonl"))?;
    if tasks.is_empty() && triggers.is_empty() {
        return Err(invalid(
            "skillset eval requires tasks.jsonl or triggers.jsonl cases",
        ));
    }
    for task in &tasks {
        if task
            .value
            .prompt_text()
            .is_none_or(|prompt| prompt.trim().is_empty())
        {
            return Err(invalid("skillset task case requires a nonempty prompt"));
        }
    }

    let mut sources = Vec::new();
    for member in members {
        enforce_skill_safety(ctx, member, "safe-capture")?;
        let path = ctx.skill_path(member).join("SKILL.md");
        let source = fs::read_to_string(&path)
            .map_err(|err| io_failure("skillset_eval_source_read", &path, err))?;
        sources.push(format!("## Member skill: {member}\n{source}"));
    }
    let mut plan = EvalPlan::run(
        ctx,
        EvalPlanInput {
            skill: args.name.clone(),
            agent: args.agent.clone(),
            runner: runner_kind,
            baseline: EvalBaselineArg::NoSkill,
            runs: 1,
            workspace: None,
            cases_path: eval_root.join("tasks.jsonl"),
            output_path: None,
            skill_source: Some(sources.join("\n\n")),
        },
    );
    let baseline = match args.baseline {
        SkillsetEvalBaselineArg::NoSkill => "no-skill",
        SkillsetEvalBaselineArg::SingleSkills => "single-skills",
    };
    let mut report = json!({
        "status": "planned",
        "runner": runner_id(runner_kind),
        "synthetic": runner_kind == EvalRunnerArg::Mock,
        "dry_run": args.dry_run,
        "baseline": baseline,
        "members": members,
        "eval_root": eval_root,
        "task_count": tasks.len(),
        "trigger_count": triggers.len(),
    });
    if args.dry_run {
        return Ok(report);
    }
    ensure_runner_available(runner_kind)?;
    let mut runner = eval_runner(runner_kind);
    let env = runner.prepare(&plan)?;
    let execution: Result<Value, CommandFailure> = (|| {
        let mut bundle_results = Vec::new();
        let mut baselines = Vec::new();
        for task in &tasks {
            bundle_results.push(runner.run_case(
                &env,
                &plan,
                &task.value,
                &case_workspace_key(task.line, &task.value),
                EvalVariant::WithSkill,
                1,
            )?);
        }
        let mut trigger_results = Vec::new();
        for trigger in &triggers {
            trigger_results.push(runner.run_trigger_case(&env, &plan, 1, trigger)?);
        }
        let baseline_count = if args.baseline == SkillsetEvalBaselineArg::NoSkill {
            1
        } else {
            members.len()
        };
        for index in 0..baseline_count {
            let (label, variant) = if args.baseline == SkillsetEvalBaselineArg::NoSkill {
                ("no-skill", EvalVariant::WithoutSkill)
            } else {
                plan.skill = members[index].to_string();
                plan.skill_source = Some(sources[index].clone());
                (members[index], EvalVariant::WithSkill)
            };
            let mut results = Vec::new();
            for task in &tasks {
                results.push(runner.run_case(
                    &env,
                    &plan,
                    &task.value,
                    &format!(
                        "baseline-{index}-{}",
                        case_workspace_key(task.line, &task.value)
                    ),
                    variant,
                    1,
                )?);
            }
            baselines.push(
                json!({"skill": label, "pass_rate": pass_rate(&results), "results": results}),
            );
        }
        let failed = bundle_results
            .iter()
            .filter(|result| result.status == "failed")
            .count()
            + trigger_results
                .iter()
                .filter(|result| result.status == "failed")
                .count();
        let passed = bundle_results
            .iter()
            .filter(|result| result.status == "passed")
            .count()
            + trigger_results
                .iter()
                .filter(|result| result.status == "passed")
                .count();
        let skipped = bundle_results
            .iter()
            .filter(|result| result.status == "skipped")
            .count();
        report["status"] = json!(if failed > 0 {
            "failed"
        } else if passed == 0 {
            "not_evaluated"
        } else {
            "passed"
        });
        report["summary"] = json!({
            "case_count": tasks.len() + triggers.len(), "passed": passed, "failed": failed,
            "skipped": skipped, "bundle_pass_rate": pass_rate(&bundle_results),
        });
        report["runs"] =
            json!({"bundle": bundle_results, "baselines": baselines, "triggers": trigger_results});
        Ok(report)
    })();
    let cleanup = runner.cleanup(env);
    match execution {
        Err(mut failure) => {
            failure.details["cleanup"] = cleanup_to_value(&cleanup);
            Err(failure)
        }
        Ok(mut report) => {
            report["cleanup"] = cleanup_to_value(&cleanup);
            if cleanup.failed() {
                let mut failure =
                    CommandFailure::new(ErrorCode::EvalFailed, "skillset eval cleanup failed");
                failure.details = report;
                return Err(failure);
            }
            Ok(report)
        }
    }
}

fn invalid(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, message)
}
