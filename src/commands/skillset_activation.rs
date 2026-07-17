use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::{
    ActivationScope, ProjectionMethod, SkillActivateArgs, SkillDeactivateArgs, SkillsetActivateArgs,
};
use crate::envelope::Meta;
use crate::next_action_trace::observe_next_actions;
use crate::state::AppContext;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::helpers::map_registry_state;
use super::skillset_cmds::{
    SkillsetMemberRecord, SkillsetRecord, load_skillsets, skill_inventory_by_id,
    validate_skillset_id,
};
use super::{App, CommandFailure};

mod eval;

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
        let mut rollback_members = Vec::new();
        for prepared_member in &prepared.ready_members {
            let member = &prepared_member.record;
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
                    if prepared_member.rollback.should_rollback() {
                        rollback_members.push(prepared_member.rollback.clone());
                    }
                    if let Some(failure) = skillset_activation_fault(&member.skill_id) {
                        return Err(self.rollback_skillset_activation_failure(
                            skillset,
                            args,
                            results,
                            rollback_members,
                            failure,
                            request_id,
                        ));
                    }
                }
                Err(err) => {
                    if prepared_member.rollback.should_rollback() {
                        rollback_members.push(prepared_member.rollback.clone());
                    }
                    return Err(self.rollback_skillset_activation_failure(
                        skillset,
                        args,
                        results,
                        rollback_members,
                        err,
                        request_id,
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
}

#[derive(Debug)]
struct PreparedSkillsetPlan {
    plan: Vec<Value>,
    ready_members: Vec<PreparedSkillsetMember>,
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
            "optional_blocked": self.summary.optional_blocked,
            "warnings": self.summary.warnings,
        })
    }
}

#[derive(Debug)]
struct PreparedSkillsetMember {
    record: SkillsetMemberRecord,
    rollback: ActivationRollbackMember,
}

#[derive(Debug, Clone)]
struct ActivationRollbackMember {
    skill_id: String,
    deactivate: bool,
    remove_projection: bool,
    materialized_path: Option<String>,
}

impl ActivationRollbackMember {
    fn should_rollback(&self) -> bool {
        self.deactivate || self.remove_projection
    }
}

#[derive(Debug, Default)]
struct PreparedSkillsetSummary {
    members: usize,
    required_ready: usize,
    optional_ready: usize,
    blocked: usize,
    optional_blocked: usize,
    warnings: usize,
}

impl App {
    fn prepare_skillset_activation(
        &self,
        skillset: &SkillsetRecord,
        args: &SkillsetActivateArgs,
        request_id: &str,
    ) -> std::result::Result<PreparedSkillsetPlan, CommandFailure> {
        let inventory = skill_inventory_by_id(&self.ctx)?;
        let snapshot = optional_registry_snapshot(&self.ctx)?;
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
                summary.optional_blocked += 1;
                summary.warnings += 1;
                plan.push(json!({
                    "skill": member.skill_id,
                    "role": member.role,
                    "required": member.required,
                    "action": "activate",
                    "status": "skipped",
                    "error": failure_json(&err),
                    "next_actions": observe_next_actions(
                        "skillset.activate.optional_missing",
                        [format!("loom skill inspect {}", member.skill_id)],
                    ),
                }));
                continue;
            }

            let activate_args = member_activate_args(&member.skill_id, args, true);
            match self.cmd_skill_activate(&activate_args, request_id) {
                Ok((data, _meta)) => {
                    let member_plan = data["plan"].clone();
                    if member.required {
                        summary.required_ready += 1;
                    } else {
                        summary.optional_ready += 1;
                    }
                    ready_members.push(PreparedSkillsetMember {
                        record: member.clone(),
                        rollback: activation_rollback_member(
                            member,
                            &member_plan,
                            snapshot.as_ref(),
                        ),
                    });
                    plan.push(json!({
                        "skill": member.skill_id,
                        "role": member.role,
                        "required": member.required,
                        "action": "activate",
                        "status": "ready",
                        "plan": member_plan,
                        "next_actions": observe_next_actions(
                            "skillset.activate.member_ready",
                            [format!("loom skill activate {} --agent {}", member.skill_id, args.agent)],
                        ),
                    }));
                }
                Err(err) if !member.required => {
                    warnings.push(format!(
                        "optional member '{}' skipped: {}",
                        member.skill_id, err.message
                    ));
                    summary.optional_blocked += 1;
                    summary.warnings += 1;
                    plan.push(json!({
                        "skill": member.skill_id,
                        "role": member.role,
                        "required": member.required,
                        "action": "activate",
                        "status": "skipped",
                        "error": failure_json(&err),
                        "next_actions": observe_next_actions(
                            "skillset.activate.optional_failed",
                            [format!("loom skill inspect {}", member.skill_id)],
                        ),
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
        rollback_members: Vec<ActivationRollbackMember>,
        failure: CommandFailure,
        request_id: &str,
    ) -> CommandFailure {
        let mut rollback = Vec::new();
        let mut rollback_errors = Vec::new();
        let recovery_commands = rollback_members
            .iter()
            .rev()
            .map(|member| skillset_deactivate_command(&member.skill_id, args))
            .collect::<Vec<_>>();

        for member in rollback_members.iter().rev() {
            let mut member_errors = Vec::new();
            let mut deactivation_result = Value::Null;
            if member.deactivate {
                let deactivate_args = member_deactivate_args(&member.skill_id, args, false);
                match self.cmd_skill_deactivate(&deactivate_args, request_id) {
                    Ok((data, _meta)) => {
                        deactivation_result = data;
                    }
                    Err(err) => {
                        member_errors.push(json!({
                            "step": "deactivate",
                            "error": failure_json(&err),
                            "recovery_command": skillset_deactivate_command(&member.skill_id, args),
                        }));
                    }
                }
            }

            let mut projection_removed = false;
            if member.remove_projection {
                match remove_created_member_projection(&self.ctx, member) {
                    Ok(removed) => {
                        projection_removed = removed;
                    }
                    Err(err) => {
                        member_errors.push(json!({
                            "step": "remove_projection",
                            "error": failure_json(&err),
                            "recovery_command": skillset_deactivate_command(&member.skill_id, args),
                        }));
                    }
                }
            }

            if member_errors.is_empty() {
                rollback.push(json!({
                    "skill": member.skill_id,
                    "status": "rolled_back",
                    "result": deactivation_result,
                    "projection_removed": projection_removed,
                }));
            } else {
                rollback_errors.push(json!({
                    "skill": member.skill_id,
                    "errors": member_errors,
                    "recovery_command": skillset_deactivate_command(&member.skill_id, args),
                }));
                rollback.push(json!({
                    "skill": member.skill_id,
                    "status": "rollback_failed",
                    "errors": rollback_errors.last().cloned().unwrap_or(Value::Null),
                }));
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

fn optional_registry_snapshot(
    ctx: &AppContext,
) -> std::result::Result<Option<RegistrySnapshot>, CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    paths.maybe_load_snapshot().map_err(map_registry_state)
}

fn activation_rollback_member(
    member: &SkillsetMemberRecord,
    plan: &Value,
    snapshot: Option<&RegistrySnapshot>,
) -> ActivationRollbackMember {
    let binding_id = plan["binding_id"].as_str();
    let target_id = plan["target_id"].as_str();
    let materialized_path = plan["materialized_path"].as_str().map(str::to_string);
    let rule_existed = match (snapshot, binding_id, target_id) {
        (Some(snapshot), Some(binding_id), Some(target_id)) => {
            snapshot.rules.rules.iter().any(|rule| {
                rule.binding_id == binding_id
                    && rule.skill_id == member.skill_id
                    && rule.target_id == target_id
            })
        }
        _ => false,
    };
    let projection_existed = action_status(plan, "project_skill") == Some("already_satisfied")
        || materialized_path
            .as_deref()
            .is_some_and(|path| fs::symlink_metadata(path).is_ok());
    let created_activation =
        !rule_existed && action_status(plan, "upsert_rule") != Some("already_satisfied");

    ActivationRollbackMember {
        skill_id: member.skill_id.clone(),
        deactivate: created_activation,
        remove_projection: created_activation && !projection_existed,
        materialized_path,
    }
}

fn action_status<'a>(plan: &'a Value, action_name: &str) -> Option<&'a str> {
    plan["actions"]
        .as_array()?
        .iter()
        .find(|action| action["action"].as_str() == Some(action_name))?
        .get("status")?
        .as_str()
}

fn remove_created_member_projection(
    ctx: &AppContext,
    member: &ActivationRollbackMember,
) -> std::result::Result<bool, CommandFailure> {
    let Some(path) = member.materialized_path.as_deref().map(PathBuf::from) else {
        return Ok(false);
    };
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if !projection_path_is_safe_symlink(&path, &ctx.skill_path(&member.skill_id)) {
                return Err(CommandFailure::new(
                    ErrorCode::ProjectionConflict,
                    format!(
                        "projection path '{}' is not a safe Loom-owned symlink for '{}'",
                        path.display(),
                        member.skill_id
                    ),
                ));
            }
            fs::remove_file(&path).map_err(|err| {
                CommandFailure::new(
                    ErrorCode::IoError,
                    format!("failed to remove projection '{}': {}", path.display(), err),
                )
            })?;
            Ok(true)
        }
        Ok(_) => Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "projection path '{}' exists but is not a symlink",
                path.display()
            ),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(CommandFailure::new(
            ErrorCode::IoError,
            format!("failed to inspect projection '{}': {}", path.display(), err),
        )),
    }
}

fn projection_path_is_safe_symlink(path: &Path, skill_src: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(link_target) = fs::read_link(path) else {
        return false;
    };
    let actual = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(link_target)
    };
    normalize_existing_or_raw(&actual) == normalize_existing_or_raw(skill_src)
}

fn normalize_existing_or_raw(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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

#[cfg(debug_assertions)]
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

#[cfg(not(debug_assertions))]
fn skillset_activation_fault(_skill: &str) -> Option<CommandFailure> {
    None
}
