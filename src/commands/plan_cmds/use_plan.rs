use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{PlanUseArgs, UseArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::next_action_trace::observe_next_actions;
use crate::types::ErrorCode;

use super::super::helpers::{
    agent_kind_as_str, map_arg, map_git, map_io, projection_method_as_str, shell_arg,
    validate_skill_name,
};
use super::super::provenance::skill_tree_digest;
use super::super::skill_policy::{SkillPolicyReport, evaluate_skill_policy};
use super::{App, CommandFailure, PLAN_PROTOCOL_VERSION, PLAN_SCHEMA_VERSION};

impl App {
    pub(super) fn cmd_plan_use(
        &self,
        args: &PlanUseArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        if !self.ctx.skill_path(&args.skill).is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }
        let registry_head = gitops::head(&self.ctx).map_err(map_git)?;
        let source_digest = skill_tree_digest(&self.ctx.skill_path(&args.skill)).map_err(map_io)?;
        let root = canonical_root(&self.ctx.root)?;
        let use_args = use_args_from_plan(args, false)?;
        let (use_plan, _) = self.cmd_use(&use_args, "")?;
        let policy = evaluate_skill_policy(&self.ctx, &args.skill, "safe-capture")?;
        let required_approvals = required_approvals(&policy);
        let risks = policy_risks(&policy);
        let plan_id = format!("plan_{}", Uuid::new_v4().simple());
        let safe_to_apply = required_approvals.is_empty()
            && !risks.iter().any(|risk| risk["blocks_apply"] == json!(true));
        let apply_command = apply_command(&self.ctx.root, &plan_id, &required_approvals);

        Ok((
            json!({
                "protocol_version": PLAN_PROTOCOL_VERSION,
                "schema_version": PLAN_SCHEMA_VERSION,
                "plan_id": plan_id,
                "operation": "use",
                "safe_to_apply": safe_to_apply,
                "effects": use_plan["steps"].clone(),
                "conflicts": [],
                "risks": risks,
                "required_approvals": required_approvals,
                "recovery": { "rollback_supported": true },
                "guards": {
                    "root": root,
                    "registry_head": registry_head,
                    "skill": args.skill,
                    "source_digest": source_digest,
                    "agents": args.agents.iter().map(|agent| agent_kind_as_str(*agent)).collect::<Vec<_>>(),
                    "workspace": use_args.workspace.as_ref().map(|path| path.display().to_string()),
                    "scope": "project",
                    "method": projection_method_as_str(args.method),
                    "target_root": use_args.target_root.as_ref().map(|path| path.display().to_string()),
                },
                "use_args": serde_json::to_value(&use_args).map_err(map_io)?,
                "next_actions": observe_next_actions(
                    "plan.use.response",
                    [format!("review this durable plan, then run `{}`", apply_command)],
                ),
            }),
            Meta::default(),
        ))
    }
}

fn use_args_from_plan(
    args: &PlanUseArgs,
    apply: bool,
) -> std::result::Result<UseArgs, CommandFailure> {
    let workspace = match args.scope {
        crate::cli::UseScope::User => args
            .workspace
            .as_ref()
            .map(|path| absolute_path(path))
            .transpose()?,
        crate::cli::UseScope::Project => Some(match args.workspace.as_ref() {
            Some(path) => absolute_path(path)?,
            None => current_dir()?,
        }),
    };
    let target_root = args
        .target_root
        .as_ref()
        .map(|path| absolute_path(path))
        .transpose()?;
    Ok(UseArgs {
        skill: args.skill.clone(),
        agents: args.agents.clone(),
        scope: args.scope,
        workspace,
        profile: args.profile.clone(),
        method: args.method,
        target_root,
        adopt: false,
        apply,
    })
}

pub(super) fn required_approvals(policy: &SkillPolicyReport) -> Vec<String> {
    let mut approvals = BTreeSet::new();
    if policy.capabilities.filesystem.contains_key("write") {
        approvals.insert("filesystem-write".to_string());
    }
    if !policy.capabilities.shell.is_empty() {
        approvals.insert("shell".to_string());
    }
    if !policy.capabilities.network.is_empty() {
        approvals.insert("network".to_string());
    }
    if !policy.capabilities.secrets.is_empty() {
        approvals.insert("secrets".to_string());
    }
    if policy.summary.high_risk_count > 0 {
        approvals.insert("policy-high-risk".to_string());
    }
    approvals.into_iter().collect()
}

pub(super) fn policy_risks(policy: &SkillPolicyReport) -> Vec<Value> {
    policy
        .findings
        .iter()
        .map(|finding| {
            json!({
                "code": finding.id,
                "risk_level": finding.risk_level,
                "blocks_apply": finding.blocks_projection,
                "details": finding.details,
            })
        })
        .collect()
}

fn apply_command(root: &Path, plan_id: &str, approvals: &[String]) -> String {
    let mut command = format!(
        "loom --json --root {} apply {} --idempotency-key <key>",
        shell_arg(root),
        shell_arg(plan_id)
    );
    for approval in approvals {
        command.push_str(&format!(" --approve {}", shell_arg(approval)));
    }
    command
}

pub(super) fn canonical_root(root: &Path) -> std::result::Result<String, CommandFailure> {
    Ok(fs::canonicalize(root)
        .map_err(map_io)?
        .display()
        .to_string())
}

fn absolute_path(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(current_dir()?.join(path))
    }
}

fn current_dir() -> std::result::Result<PathBuf, CommandFailure> {
    std::env::current_dir().map_err(|err| {
        CommandFailure::new(
            ErrorCode::IoError,
            format!("failed to resolve current workspace: {}", err),
        )
    })
}
