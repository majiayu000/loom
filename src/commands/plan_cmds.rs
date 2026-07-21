use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{ApplyArgs, PlanCommand, PlanUseArgs, UseArgs};
use crate::core::convergence::stored_plan_digest;
use crate::envelope::Meta;
use crate::gitops;
use crate::next_action_trace::observe_next_actions;
use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

use super::event_store::{CommandEventRow, read_command_events};
use super::helpers::{
    agent_kind_as_str, map_arg, map_git, map_io, projection_method_as_str, shell_arg,
    validate_skill_name,
};
use super::provenance::skill_tree_digest;
use super::skill_policy::evaluate_skill_policy;
use super::{App, CommandFailure};

mod converge;
mod convergence_transaction;

const PLAN_PROTOCOL_VERSION: &str = "1.0";
const PLAN_SCHEMA_VERSION: &str = "1.0";

impl App {
    pub fn cmd_plan(
        &self,
        command: &PlanCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            PlanCommand::Converge(args) => self.cmd_plan_converge(args),
            PlanCommand::Use(args) => self.cmd_plan_use(args),
        }
    }

    pub fn cmd_apply(
        &self,
        args: &ApplyArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_plan_id(&args.plan_id)?;
        validate_idempotency_key(&args.idempotency_key)?;
        let events = read_command_events(&self.ctx).map_err(map_io)?;
        let stored = find_plan(&events, &args.plan_id).ok_or_else(|| {
            plan_failure(
                ErrorCode::ArgInvalid,
                format!("plan '{}' not found", args.plan_id),
                "PLAN_NOT_FOUND",
                false,
                vec!["create a fresh durable plan".to_string()],
                None,
            )
        })?;
        validate_stored_plan_metadata(&stored)?;
        if stored.kind == StoredPlanKind::Converge {
            validate_confirmed_plan_digest(
                stored.plan,
                stored.cursor,
                args.plan_digest.as_deref(),
            )?;
        }

        let idempotency_key_digest = idempotency_key_digest(&args.idempotency_key);
        if let Some(replay) = find_prior_apply(&events, &args.plan_id, &idempotency_key_digest)? {
            if stored.kind == StoredPlanKind::Converge && remote_transport_is_pending(&replay) {
                let output = convergence_transaction::retry_remote_transport(
                    self,
                    stored.plan,
                    stored.cursor,
                    &idempotency_key_digest,
                    replay["applied"].clone(),
                )?;
                return Ok((output, Meta::default()));
            }
            return Ok((replay, Meta::default()));
        }
        if let Some(conflict) = find_key_conflict(&events, &args.plan_id, &idempotency_key_digest) {
            return Err(plan_failure(
                ErrorCode::DependencyConflict,
                "idempotency key was already used for a different plan",
                "IDEMPOTENCY_KEY_REUSED",
                false,
                vec!["retry with a new idempotency key".to_string()],
                Some(conflict.cursor),
            ));
        }
        if stored.kind == StoredPlanKind::Converge {
            let output = convergence_transaction::apply_convergence(
                self,
                stored.plan,
                stored.cursor,
                &idempotency_key_digest,
                request_id,
            )?;
            return Ok((output, Meta::default()));
        }

        validate_plan_guards(stored.plan, stored.cursor, &args.approvals, &self.ctx.root)?;

        let mut use_args = plan_use_args(stored.plan)?;
        use_args.apply = true;
        let (use_data, mut meta) = self.cmd_use(&use_args, request_id)?;
        let rollback_commands = collect_rollback_commands(&use_data);
        Ok((
            json!({
                "protocol_version": PLAN_PROTOCOL_VERSION,
                "schema_version": PLAN_SCHEMA_VERSION,
                "plan_id": args.plan_id,
                "idempotency_key_digest": idempotency_key_digest,
                "idempotent_replay": false,
                "plan_event_cursor": stored.cursor,
                "applied": use_data,
                "recovery": {
                    "rollback_supported": true,
                    "rollback_commands": rollback_commands,
                },
            }),
            {
                meta.warnings.push(format!(
                    "applied durable plan {} from event cursor {}",
                    args.plan_id, stored.cursor
                ));
                meta
            },
        ))
    }

    fn cmd_plan_use(
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

struct StoredPlan<'a> {
    cursor: usize,
    plan: &'a Value,
    kind: StoredPlanKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoredPlanKind {
    Use,
    Converge,
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

fn plan_use_args(plan: &Value) -> std::result::Result<UseArgs, CommandFailure> {
    let Some(value) = plan.get("use_args") else {
        return Err(plan_failure(
            ErrorCode::StateCorrupt,
            "stored plan is missing use_args",
            "PLAN_CORRUPT",
            false,
            vec!["create a fresh plan with `loom plan use ...`".to_string()],
            None,
        ));
    };
    serde_json::from_value::<UseArgs>(value.clone()).map_err(|err| {
        plan_failure(
            ErrorCode::StateCorrupt,
            format!("stored plan use_args are invalid: {err}"),
            "PLAN_CORRUPT",
            false,
            vec!["create a fresh plan with `loom plan use ...`".to_string()],
            None,
        )
    })
}

fn find_plan<'a>(events: &'a [CommandEventRow], plan_id: &str) -> Option<StoredPlan<'a>> {
    events.iter().rev().find_map(|row| {
        let kind = match row.event.cmd.as_str() {
            "plan.use" => StoredPlanKind::Use,
            "plan.converge" => StoredPlanKind::Converge,
            _ => return None,
        };
        if row.event.status != "succeeded" {
            return None;
        }
        let plan = row.event.output.as_ref()?;
        (plan["plan_id"].as_str() == Some(plan_id)).then_some(StoredPlan {
            cursor: row.cursor,
            plan,
            kind,
        })
    })
}

fn validate_stored_plan_metadata(
    stored: &StoredPlan<'_>,
) -> std::result::Result<(), CommandFailure> {
    if stored.kind == StoredPlanKind::Converge
        && stored.plan["operation"] == json!("converge")
        && stored.plan["schema_version"] == json!("1.1")
    {
        return Err(plan_failure(
            ErrorCode::SchemaMismatch,
            "stored convergence plan schema 1.1 cannot be applied by the schema 1.2 executor",
            "PLAN_SCHEMA_UNSUPPORTED",
            false,
            vec!["create and review a fresh convergence plan".to_string()],
            Some(stored.cursor),
        ));
    }
    let valid = match stored.kind {
        StoredPlanKind::Use => {
            stored.plan["operation"] == json!("use")
                && stored.plan["schema_version"] == json!(PLAN_SCHEMA_VERSION)
                && stored.plan["requires_digest_confirmation"] != json!(true)
        }
        StoredPlanKind::Converge => {
            stored.plan["operation"] == json!("converge")
                && stored.plan["schema_version"] == json!("1.2")
                && stored.plan["requires_digest_confirmation"] == json!(true)
                && stored.plan["execution_enabled"] == json!(true)
        }
    };
    if valid {
        return Ok(());
    }
    Err(plan_failure(
        ErrorCode::StateCorrupt,
        "stored plan metadata does not match its command event kind",
        "PLAN_CORRUPT",
        false,
        vec!["discard the corrupted plan and create a fresh durable plan".to_string()],
        Some(stored.cursor),
    ))
}

fn validate_confirmed_plan_digest(
    plan: &Value,
    cursor: usize,
    confirmed: Option<&str>,
) -> std::result::Result<(), CommandFailure> {
    let expected = plan["plan_digest"].as_str().ok_or_else(|| {
        plan_failure(
            ErrorCode::StateCorrupt,
            "stored convergence plan is missing plan_digest",
            "PLAN_CORRUPT",
            false,
            vec!["create a fresh convergence plan".to_string()],
            Some(cursor),
        )
    })?;
    let recomputed = stored_plan_digest(plan)
        .ok_or_else(|| {
            plan_failure(
                ErrorCode::StateCorrupt,
                "stored convergence plan is missing digest-covered fields",
                "PLAN_CORRUPT",
                false,
                vec!["create a fresh convergence plan".to_string()],
                Some(cursor),
            )
        })?
        .map_err(|err| {
            plan_failure(
                ErrorCode::StateCorrupt,
                format!("stored convergence plan digest could not be recomputed: {err}"),
                "PLAN_CORRUPT",
                false,
                vec!["create a fresh convergence plan".to_string()],
                Some(cursor),
            )
        })?;
    if expected != recomputed {
        return Err(plan_failure(
            ErrorCode::StateCorrupt,
            "stored convergence plan payload does not match its plan_digest",
            "PLAN_DIGEST_INVALID",
            false,
            vec!["discard the corrupted plan and create a fresh convergence plan".to_string()],
            Some(cursor),
        ));
    }
    let Some(confirmed) = confirmed.filter(|value| !value.trim().is_empty()) else {
        return Err(plan_failure(
            ErrorCode::ArgInvalid,
            "--plan-digest is required for convergence plans",
            "PLAN_DIGEST_REQUIRED",
            true,
            vec!["rerun apply with the exact plan_digest returned by plan converge".to_string()],
            Some(cursor),
        ));
    };
    if confirmed != expected {
        return Err(plan_failure(
            ErrorCode::DependencyConflict,
            "confirmed plan digest does not match the stored convergence plan",
            "PLAN_DIGEST_MISMATCH",
            false,
            vec!["review the stored plan and use its exact plan_digest".to_string()],
            Some(cursor),
        ));
    }
    Ok(())
}

fn find_prior_apply(
    events: &[CommandEventRow],
    plan_id: &str,
    idempotency_key_digest: &str,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let Some(row) = events.iter().rev().find(|row| {
        row.event.cmd == "apply"
            && row.event.status == "succeeded"
            && row
                .event
                .output
                .as_ref()
                .and_then(|data| data["plan_id"].as_str())
                == Some(plan_id)
            && row
                .event
                .output
                .as_ref()
                .and_then(|data| data["idempotency_key_digest"].as_str())
                == Some(idempotency_key_digest)
    }) else {
        return Ok(None);
    };
    let mut replay = row.event.output.clone().ok_or_else(|| {
        plan_failure(
            ErrorCode::StateCorrupt,
            "prior apply event is missing output",
            "APPLY_EVENT_CORRUPT",
            false,
            vec!["retry with a new idempotency key".to_string()],
            Some(row.cursor),
        )
    })?;
    scrub_legacy_apply_output(&mut replay);
    replay["idempotent_replay"] = json!(true);
    replay["replayed_from_event_cursor"] = json!(row.cursor);
    Ok(Some(replay))
}

fn scrub_legacy_apply_output(output: &mut Value) {
    if let Some(recovery) = output.get_mut("recovery").and_then(Value::as_object_mut) {
        recovery.remove("rollback_token");
    }
}

fn remote_transport_is_pending(output: &Value) -> bool {
    output["completion_blockers"]
        .as_array()
        .is_some_and(|blockers| {
            blockers
                .iter()
                .any(|blocker| blocker == "registry.remote_pending")
        })
}

fn find_key_conflict<'a>(
    events: &'a [CommandEventRow],
    plan_id: &str,
    idempotency_key_digest: &str,
) -> Option<&'a CommandEventRow> {
    events.iter().rev().find(|row| {
        row.event.cmd == "apply"
            && row.event.status == "succeeded"
            && row
                .event
                .output
                .as_ref()
                .and_then(|data| data["idempotency_key_digest"].as_str())
                == Some(idempotency_key_digest)
            && row
                .event
                .output
                .as_ref()
                .and_then(|data| data["plan_id"].as_str())
                != Some(plan_id)
    })
}

fn validate_plan_guards(
    plan: &Value,
    cursor: usize,
    approvals: &[String],
    root: &Path,
) -> std::result::Result<(), CommandFailure> {
    let guards = &plan["guards"];
    let current_root = canonical_root(root)?;
    if guards["root"].as_str() != Some(current_root.as_str()) {
        return Err(plan_failure(
            ErrorCode::DependencyConflict,
            "plan root does not match current --root",
            "PLAN_ROOT_MISMATCH",
            false,
            vec!["recreate the plan under the current --root".to_string()],
            Some(cursor),
        ));
    }
    let current_head =
        gitops::head(&crate::state::AppContext::new(Some(root.to_path_buf())).map_err(map_io)?)
            .map_err(map_git)?;
    if guards["registry_head"].as_str() != Some(current_head.as_str()) {
        return Err(plan_failure(
            ErrorCode::DependencyConflict,
            "registry HEAD changed after the plan was created",
            "PLAN_STALE",
            false,
            vec!["create a fresh plan before applying".to_string()],
            Some(cursor),
        ));
    }
    let skill = guards["skill"].as_str().ok_or_else(|| {
        plan_failure(
            ErrorCode::StateCorrupt,
            "stored plan is missing guards.skill",
            "PLAN_CORRUPT",
            false,
            vec!["create a fresh plan with `loom plan use ...`".to_string()],
            Some(cursor),
        )
    })?;
    let current_digest = skill_tree_digest(&root.join("skills").join(skill)).map_err(map_io)?;
    if guards["source_digest"].as_str() != Some(current_digest.as_str()) {
        return Err(plan_failure(
            ErrorCode::DependencyConflict,
            "skill source digest changed after the plan was created",
            "PLAN_SOURCE_DRIFT",
            false,
            vec!["create a fresh plan before applying".to_string()],
            Some(cursor),
        ));
    }
    let required = plan["required_approvals"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let approved = approvals
        .iter()
        .map(|approval| approval.trim())
        .filter(|approval| !approval.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let missing = required.difference(&approved).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(plan_failure(
            ErrorCode::PolicyBlocked,
            format!("plan requires approval token(s): {}", missing.join(", ")),
            "APPROVAL_REQUIRED",
            true,
            vec![format!(
                "rerun apply with {}",
                missing
                    .iter()
                    .map(|token| format!("--approve {}", shell_arg(token)))
                    .collect::<Vec<_>>()
                    .join(" ")
            )],
            Some(cursor),
        ));
    }
    Ok(())
}

fn required_approvals(policy: &super::skill_policy::SkillPolicyReport) -> Vec<String> {
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

fn policy_risks(policy: &super::skill_policy::SkillPolicyReport) -> Vec<Value> {
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

fn collect_rollback_commands(use_data: &Value) -> Vec<String> {
    use_data["applied"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| item["rollback_commands"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn canonical_root(root: &Path) -> std::result::Result<String, CommandFailure> {
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

fn validate_plan_id(plan_id: &str) -> std::result::Result<(), CommandFailure> {
    if plan_id.strip_prefix("plan_").is_none()
        || plan_id
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "plan_id must start with plan_ and contain only ASCII letters, digits, and underscores",
        ));
    }
    Ok(())
}

fn validate_idempotency_key(key: &str) -> std::result::Result<(), CommandFailure> {
    let trimmed = key.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--idempotency-key must not be empty or option-like",
        ));
    }
    Ok(())
}

fn idempotency_key_digest(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

fn plan_failure(
    code: ErrorCode,
    message: impl Into<String>,
    conflict_code: &str,
    retryable: bool,
    suggested_actions: Vec<String>,
    event_cursor: Option<usize>,
) -> CommandFailure {
    let mut failure = CommandFailure::new(code, message);
    failure.details = json!({
        "retryable": retryable,
        "conflict": { "code": conflict_code },
        "event_cursor": event_cursor,
        "suggested_actions": suggested_actions,
    });
    failure
}
