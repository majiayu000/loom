use serde_json::{Value, json};

use crate::cli::SkillEvalOfflineArgs;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::{
    EvalRun, TaskCase, TriggerCase, eval_agents, evaluate_run, read_jsonl, skill_eval_version,
    summarize_runs,
};
use crate::commands::CommandFailure;
use crate::commands::helpers::{map_arg, validate_skill_name};

pub(crate) struct OfflineEvalResult {
    pub(crate) report: Value,
    pub(crate) warnings: Vec<String>,
    pub(crate) runs: Vec<EvalRun>,
    pub(crate) failed: usize,
}

pub(crate) fn build_skill_eval_offline_report(
    ctx: &AppContext,
    args: &SkillEvalOfflineArgs,
) -> std::result::Result<OfflineEvalResult, CommandFailure> {
    validate_skill_name(&args.skill).map_err(map_arg)?;
    let skill_path = ctx.skill_path(&args.skill);
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

    let version = skill_eval_version(ctx, &args.skill);

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
        "schema_version": super::EVAL_SCHEMA_VERSION,
        "skill": args.skill,
        "mode": "offline_fixture",
        "skill_version": version,
        "eval_root": evals_dir.display().to_string(),
        "matrix": agents,
        "summary": summary,
        "runs": &runs,
        "security_model": {
            "eval_success_is_safety_guarantee": false,
            "note": "Eval success is quality evidence only. It does not prove the skill is safe, sandboxed, or free of prompt-injection risk."
        }
    });
    Ok(OfflineEvalResult {
        report,
        warnings,
        runs,
        failed,
    })
}
