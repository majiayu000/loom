use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::cli::{
    ActivationScope, ProjectionMethod, SkillActivateArgs, SkillDeactivateArgs,
    SkillEvalOfflineArgs, SkillsetActivateArgs, SkillsetEvalArgs, SkillsetEvalBaselineArg,
};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::skill_eval::build_skill_eval_offline_report;
use super::skillset_cmds::{
    SkillsetMemberRecord, SkillsetRecord, load_skillsets, skill_inventory_by_id,
    validate_skillset_id,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_skillset_activate(
        &self,
        args: &SkillsetActivateArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        let file = load_skillsets(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let prepared = self.prepare_skillset_activation(skillset, args, request_id)?;

        if args.dry_run {
            return Ok((
                json!({
                    "skillset": skillset.id,
                    "agent": args.agent,
                    "scope": scope_label(args.scope),
                    "dry_run": true,
                    "ready": prepared.summary.blocked == 0,
                    "activation_plan": prepared.plan,
                    "summary": prepared.summary_json(),
                }),
                Meta {
                    warnings: prepared.warnings,
                    ..Meta::default()
                },
            ));
        }

        let mut results = Vec::new();
        let mut activated = Vec::new();
        for member in &prepared.ready_members {
            let activate_args = member_activate_args(&member.skill_id, args, false);
            match self.cmd_skill_activate(&activate_args, request_id) {
                Ok((data, _meta)) => {
                    let noop = data["noop"].as_bool().unwrap_or(false);
                    results.push(json!({
                        "skill": member.skill_id,
                        "status": "activated",
                        "noop": noop,
                        "result": data,
                    }));
                    if !noop {
                        activated.push(member.skill_id.clone());
                    }
                    if let Some(failure) = skillset_activation_fault(&member.skill_id) {
                        return Err(self.rollback_skillset_activation_failure(
                            skillset, args, results, activated, failure, request_id,
                        ));
                    }
                }
                Err(err) => {
                    return Err(self.rollback_skillset_activation_failure(
                        skillset, args, results, activated, err, request_id,
                    ));
                }
            }
        }

        Ok((
            json!({
                "skillset": skillset.id,
                "agent": args.agent,
                "scope": scope_label(args.scope),
                "dry_run": false,
                "ready": true,
                "activation_plan": prepared.plan,
                "results": results,
                "summary": prepared.summary_json(),
            }),
            Meta {
                warnings: prepared.warnings,
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_skillset_deactivate(
        &self,
        args: &SkillsetActivateArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        let file = load_skillsets(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let mut plans = Vec::new();
        let mut results = Vec::new();
        let mut warnings = Vec::new();

        for member in &skillset.members {
            let deactivate_args = member_deactivate_args(&member.skill_id, args, true);
            match self.cmd_skill_deactivate(&deactivate_args, request_id) {
                Ok((data, _meta)) => plans.push(json!({
                    "skill": member.skill_id,
                    "required": member.required,
                    "status": "ready",
                    "plan": data["plan"].clone(),
                })),
                Err(err) if !member.required => {
                    warnings.push(format!(
                        "optional member '{}' could not be planned for deactivation: {}",
                        member.skill_id, err.message
                    ));
                    plans.push(json!({
                        "skill": member.skill_id,
                        "required": member.required,
                        "status": "skipped",
                        "error": failure_json(&err),
                    }));
                }
                Err(err) => return Err(member_required_failure("deactivation", member, err)),
            }
        }

        if args.dry_run {
            return Ok((
                json!({
                    "skillset": skillset.id,
                    "agent": args.agent,
                    "scope": scope_label(args.scope),
                    "dry_run": true,
                    "deactivation_plan": plans,
                }),
                Meta {
                    warnings,
                    ..Meta::default()
                },
            ));
        }

        for member in &skillset.members {
            let deactivate_args = member_deactivate_args(&member.skill_id, args, false);
            match self.cmd_skill_deactivate(&deactivate_args, request_id) {
                Ok((data, _meta)) => results.push(json!({
                    "skill": member.skill_id,
                    "status": "deactivated",
                    "noop": data["noop"].as_bool().unwrap_or(false),
                    "result": data,
                })),
                Err(err) if !member.required => {
                    warnings.push(format!(
                        "optional member '{}' could not be deactivated: {}",
                        member.skill_id, err.message
                    ));
                    results.push(json!({
                        "skill": member.skill_id,
                        "status": "skipped",
                        "error": failure_json(&err),
                    }));
                }
                Err(err) => return Err(member_required_failure("deactivation", member, err)),
            }
        }

        Ok((
            json!({
                "skillset": skillset.id,
                "agent": args.agent,
                "scope": scope_label(args.scope),
                "dry_run": false,
                "deactivation_plan": plans,
                "results": results,
            }),
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }

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
                    failed += result.failed;
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

#[derive(Debug)]
struct PreparedSkillsetPlan {
    plan: Vec<Value>,
    ready_members: Vec<SkillsetMemberRecord>,
    warnings: Vec<String>,
    summary: PreparedSkillsetSummary,
}

impl PreparedSkillsetPlan {
    fn summary_json(&self) -> Value {
        json!({
            "members": self.summary.members,
            "required_ready": self.summary.required_ready,
            "optional_ready": self.summary.optional_ready,
            "blocked": self.summary.blocked,
            "warnings": self.summary.warnings,
        })
    }
}

#[derive(Debug, Default)]
struct PreparedSkillsetSummary {
    members: usize,
    required_ready: usize,
    optional_ready: usize,
    blocked: usize,
    warnings: usize,
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

impl App {
    fn prepare_skillset_activation(
        &self,
        skillset: &SkillsetRecord,
        args: &SkillsetActivateArgs,
        request_id: &str,
    ) -> std::result::Result<PreparedSkillsetPlan, CommandFailure> {
        let inventory = skill_inventory_by_id(&self.ctx)?;
        let mut plan = Vec::new();
        let mut ready_members = Vec::new();
        let mut warnings = Vec::new();
        let mut summary = PreparedSkillsetSummary {
            members: skillset.members.len(),
            ..PreparedSkillsetSummary::default()
        };

        for member in &skillset.members {
            if !inventory.contains_key(&member.skill_id) {
                let err = CommandFailure::new(
                    ErrorCode::SkillNotFound,
                    format!("member skill '{}' is missing", member.skill_id),
                );
                if member.required {
                    return Err(member_required_failure("activation", member, err));
                }
                warnings.push(format!(
                    "optional member '{}' skipped because the skill is missing",
                    member.skill_id
                ));
                summary.blocked += 1;
                summary.warnings += 1;
                plan.push(json!({
                    "skill": member.skill_id,
                    "role": member.role,
                    "required": member.required,
                    "action": "activate",
                    "status": "skipped",
                    "error": failure_json(&err),
                    "next_actions": [format!("loom skill inspect {}", member.skill_id)],
                }));
                continue;
            }

            let activate_args = member_activate_args(&member.skill_id, args, true);
            match self.cmd_skill_activate(&activate_args, request_id) {
                Ok((data, _meta)) => {
                    if member.required {
                        summary.required_ready += 1;
                    } else {
                        summary.optional_ready += 1;
                    }
                    ready_members.push(member.clone());
                    plan.push(json!({
                        "skill": member.skill_id,
                        "role": member.role,
                        "required": member.required,
                        "action": "activate",
                        "status": "ready",
                        "plan": data["plan"].clone(),
                        "next_actions": [format!("loom skill activate {} --agent {}", member.skill_id, args.agent)],
                    }));
                }
                Err(err) if !member.required => {
                    warnings.push(format!(
                        "optional member '{}' skipped: {}",
                        member.skill_id, err.message
                    ));
                    summary.blocked += 1;
                    summary.warnings += 1;
                    plan.push(json!({
                        "skill": member.skill_id,
                        "role": member.role,
                        "required": member.required,
                        "action": "activate",
                        "status": "skipped",
                        "error": failure_json(&err),
                        "next_actions": [format!("loom skill inspect {}", member.skill_id)],
                    }));
                }
                Err(err) => return Err(member_required_failure("activation", member, err)),
            }
        }

        Ok(PreparedSkillsetPlan {
            plan,
            ready_members,
            warnings,
            summary,
        })
    }

    fn rollback_skillset_activation_failure(
        &self,
        skillset: &SkillsetRecord,
        args: &SkillsetActivateArgs,
        results: Vec<Value>,
        activated: Vec<String>,
        failure: CommandFailure,
        request_id: &str,
    ) -> CommandFailure {
        let mut rollback = Vec::new();
        let mut rollback_errors = Vec::new();
        let recovery_commands = activated
            .iter()
            .rev()
            .map(|skill| skillset_deactivate_command(skill, args))
            .collect::<Vec<_>>();

        for skill in activated.iter().rev() {
            let deactivate_args = member_deactivate_args(skill, args, false);
            match self.cmd_skill_deactivate(&deactivate_args, request_id) {
                Ok((data, _meta)) => rollback.push(json!({
                    "skill": skill,
                    "status": "rolled_back",
                    "result": data,
                })),
                Err(err) => {
                    rollback_errors.push(json!({
                        "skill": skill,
                        "error": failure_json(&err),
                        "recovery_command": skillset_deactivate_command(skill, args),
                    }));
                    rollback.push(json!({
                        "skill": skill,
                        "status": "rollback_failed",
                        "error": failure_json(&err),
                    }));
                }
            }
        }

        let mut out = CommandFailure::new(
            failure.code,
            format!(
                "skillset '{}' activation failed after partial application",
                skillset.id
            ),
        );
        out.details = json!({
            "original_error": failure_json(&failure),
            "results_before_failure": results,
            "rollback": rollback,
            "rollback_complete": rollback_errors.is_empty(),
            "rollback_errors": rollback_errors,
            "recovery_commands": recovery_commands,
        });
        out
    }
}

fn member_activate_args(
    skill: &str,
    args: &SkillsetActivateArgs,
    dry_run: bool,
) -> SkillActivateArgs {
    SkillActivateArgs {
        skill: skill.to_string(),
        agent: args.agent.clone(),
        scope: args.scope,
        workspace: args.workspace.clone(),
        profile: args.profile.clone(),
        target: None,
        method: ProjectionMethod::Symlink,
        compiled: false,
        artifact: None,
        dry_run,
    }
}

fn member_deactivate_args(
    skill: &str,
    args: &SkillsetActivateArgs,
    dry_run: bool,
) -> SkillDeactivateArgs {
    SkillDeactivateArgs {
        skill: skill.to_string(),
        agent: args.agent.clone(),
        scope: args.scope,
        workspace: args.workspace.clone(),
        profile: args.profile.clone(),
        target: None,
        dry_run,
    }
}

fn member_required_failure(
    action: &str,
    member: &SkillsetMemberRecord,
    err: CommandFailure,
) -> CommandFailure {
    let mut failure = CommandFailure::new(
        err.code,
        format!(
            "required member '{}' is not ready for skillset {}: {}",
            member.skill_id,
            action,
            err.message.clone()
        ),
    );
    failure.details = json!({
        "member": {
            "skill_id": member.skill_id,
            "role": member.role,
            "required": member.required,
        },
        "original_error": failure_json(&err),
    });
    failure
}

fn failure_json(err: &CommandFailure) -> Value {
    json!({
        "code": err.code.as_str(),
        "message": err.message.clone(),
        "details": err.details.clone(),
        "next_actions": err.next_actions.clone(),
    })
}

fn scope_label(scope: ActivationScope) -> &'static str {
    match scope {
        ActivationScope::User => "user",
        ActivationScope::Project => "project",
    }
}

fn skillset_deactivate_command(skill: &str, args: &SkillsetActivateArgs) -> String {
    let mut command = format!(
        "loom skill deactivate {} --agent {} --scope {}",
        skill,
        args.agent,
        scope_label(args.scope)
    );
    if let Some(workspace) = &args.workspace {
        command.push_str(&format!(" --workspace {}", workspace.display()));
    }
    if let Some(profile) = &args.profile {
        command.push_str(&format!(" --profile {}", profile));
    }
    command
}

fn skillset_activation_fault(skill: &str) -> Option<CommandFailure> {
    let raw = std::env::var("LOOM_SKILLSET_ACTIVATE_FAULT_INJECT").ok()?;
    if raw == format!("after:{skill}") {
        return Some(CommandFailure::new(
            ErrorCode::InternalError,
            format!("fault injected after activating {}", skill),
        ));
    }
    None
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
        "next_actions": [
            "track a follow-up runner for skillsets/<name>/evals/"
        ],
    })
}

fn usize_field(value: &Value, key: &str) -> usize {
    value[key].as_u64().unwrap_or(0) as usize
}

fn u64_field(value: &Value, key: &str) -> u64 {
    value[key].as_u64().unwrap_or(0)
}
