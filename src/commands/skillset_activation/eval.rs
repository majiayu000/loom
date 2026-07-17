use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::cli::{SkillEvalOfflineArgs, SkillsetEvalArgs, SkillsetEvalBaselineArg};
use crate::commands::skill_eval::build_skill_eval_offline_report;
use crate::commands::skillset_cmds::{load_skillsets, validate_skillset_id};
use crate::commands::{App, CommandFailure};
use crate::envelope::Meta;
use crate::next_action_trace::observe_next_actions;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::{failure_json, member_required_failure};

impl App {
    pub fn cmd_skillset_eval(
        &self,
        args: &SkillsetEvalArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        let file = load_skillsets(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let mut members = Vec::new();
        let mut warnings = Vec::new();
        let mut summary = EvalAggregateSummary::default();
        let mut failed = 0usize;

        for member in &skillset.members {
            let eval_args = SkillEvalOfflineArgs {
                skill: member.skill_id.clone(),
                agent: Some(args.agent.clone()),
                matrix: Vec::new(),
                model: None,
            };
            match build_skill_eval_offline_report(&self.ctx, &eval_args) {
                Ok(result) => {
                    let member_summary = result.report["summary"].clone();
                    summary.add_json_summary(&member_summary);
                    if member.required {
                        failed += result.failed;
                    }
                    warnings.extend(result.warnings);
                    members.push(json!({
                        "skill": member.skill_id,
                        "required": member.required,
                        "status": if result.failed == 0 { "passed" } else { "failed" },
                        "summary": member_summary,
                        "report": result.report,
                    }));
                }
                Err(err) if !member.required => {
                    warnings.push(format!(
                        "optional member '{}' eval skipped: {}",
                        member.skill_id, err.message
                    ));
                    members.push(json!({
                        "skill": member.skill_id,
                        "required": member.required,
                        "status": "skipped",
                        "error": failure_json(&err),
                    }));
                }
                Err(err) => return Err(member_required_failure("eval", member, err)),
            }
        }

        let report = json!({
            "schema_version": 1,
            "skillset": skillset.id,
            "agent": args.agent,
            "baseline": skillset_eval_baseline_label(args.baseline),
            "members": members,
            "summary": summary.to_json(),
            "end_to_end": skillset_end_to_end_status(&self.ctx, &skillset.id),
            "security_model": {
                "eval_success_is_safety_guarantee": false,
                "note": "Skillset eval aggregates member quality evidence only. It does not prove the bundle is safe, sandboxed, or free of prompt-injection risk."
            }
        });

        if failed > 0 {
            let mut failure = CommandFailure::new(
                ErrorCode::EvalFailed,
                format!("skillset eval failed with {} failing case(s)", failed),
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

#[derive(Debug, Default)]
struct EvalAggregateSummary {
    case_count: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
    token_count: u64,
    command_count: u64,
    permissions_used: BTreeSet<String>,
}

impl EvalAggregateSummary {
    fn add_json_summary(&mut self, summary: &Value) {
        self.case_count += usize_field(summary, "case_count");
        self.passed += usize_field(summary, "passed");
        self.failed += usize_field(summary, "failed");
        self.skipped += usize_field(summary, "skipped");
        self.token_count += u64_field(summary, "token_count");
        self.command_count += u64_field(summary, "command_count");
        for permission in summary["permissions_used"].as_array().into_iter().flatten() {
            if let Some(permission) = permission.as_str() {
                self.permissions_used.insert(permission.to_string());
            }
        }
    }

    fn to_json(&self) -> Value {
        let active = self.passed + self.failed;
        let aggregate_score = if active == 0 {
            None
        } else {
            Some(self.passed as f64 / active as f64)
        };
        json!({
            "case_count": self.case_count,
            "passed": self.passed,
            "failed": self.failed,
            "skipped": self.skipped,
            "aggregate_score": aggregate_score,
            "token_count": self.token_count,
            "command_count": self.command_count,
            "permissions_used": self.permissions_used.iter().collect::<Vec<_>>(),
        })
    }
}

fn skillset_eval_baseline_label(baseline: SkillsetEvalBaselineArg) -> &'static str {
    match baseline {
        SkillsetEvalBaselineArg::NoSkill => "no-skill",
        SkillsetEvalBaselineArg::SingleSkills => "single-skills",
    }
}

fn skillset_end_to_end_status(ctx: &AppContext, name: &str) -> Value {
    let evals_dir = ctx.root.join("skillsets").join(name).join("evals");
    let trigger_path = evals_dir.join("triggers.jsonl");
    let task_path = evals_dir.join("tasks.jsonl");
    if !trigger_path.is_file() && !task_path.is_file() {
        return json!({
            "status": "not_configured",
            "eval_root": evals_dir.display().to_string(),
        });
    }
    json!({
        "status": "deferred",
        "eval_root": evals_dir.display().to_string(),
        "reason": "skillset end-to-end eval fixtures are detected but this command currently aggregates member evals only",
        "next_actions": observe_next_actions(
            "skillset.eval.deferred",
            ["track a follow-up runner for skillsets/<name>/evals/"],
        ),
    })
}

fn usize_field(value: &Value, key: &str) -> usize {
    value[key].as_u64().unwrap_or(0) as usize
}

fn u64_field(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}
