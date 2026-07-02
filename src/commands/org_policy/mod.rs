mod state;

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::{
    ApprovalCommand, ApprovalDecisionArgs, ApprovalListArgs, ApprovalRequestArgs,
    OrgPolicyCheckArgs, OrgPolicyCommand, OrgPolicyInitArgs, RoleGrantArgs, RolesCommand,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::REGISTRY_SCHEMA_VERSION;
use crate::types::ErrorCode;

use super::helpers::map_lock;
use super::skill_safety::trust_metadata_for_skill;
use super::{App, CommandFailure};
use state::{
    RoleGrantRecord, RolesFile, append_approval_event, approval_decision_event,
    approval_requested_event, approval_state_json, approval_summary, canonical_action,
    commit_policy_change, current_actor, default_policy_toml, has_resolved_admin,
    load_approval_states, load_policy_document, load_roles, org_policy_digest_json,
    org_policy_path, policy_blocked, policy_json, required_roles_for_action, roles_for_subject,
    roles_json, roles_path, save_roles, shell_arg, subject_for_action, subject_has_role,
    validate_request_id, validate_role, validate_subject, write_string,
};

impl App {
    pub fn cmd_policy_org(
        &self,
        command: &OrgPolicyCommand,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            OrgPolicyCommand::Init(args) => self.cmd_policy_org_init(args, request_id),
            OrgPolicyCommand::Show => self.cmd_policy_org_show(),
            OrgPolicyCommand::Check(args) => {
                let decision = evaluate_org_policy(&self.ctx, args)?;
                Ok((json!({"policy": decision}), Meta::default()))
            }
        }
    }

    pub fn cmd_approval(
        &self,
        command: &ApprovalCommand,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            ApprovalCommand::Request(args) => self.cmd_approval_request(args, request_id),
            ApprovalCommand::List(args) => self.cmd_approval_list(args),
            ApprovalCommand::Approve(args) => self.cmd_approval_decision(args, true, request_id),
            ApprovalCommand::Reject(args) => self.cmd_approval_decision(args, false, request_id),
        }
    }

    pub fn cmd_roles(
        &self,
        command: &RolesCommand,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            RolesCommand::List => self.cmd_roles_list(),
            RolesCommand::Grant(args) => self.cmd_roles_grant(args, true, request_id),
            RolesCommand::Revoke(args) => self.cmd_roles_grant(args, false, request_id),
        }
    }

    fn cmd_policy_org_init(
        &self,
        args: &OrgPolicyInitArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let policy_path = org_policy_path(&self.ctx);
        let roles_path = roles_path(&self.ctx);
        if policy_path.exists() || roles_path.exists() {
            let policy = load_policy_document(&self.ctx)?;
            let roles = load_roles(&self.ctx)?;
            return Ok((
                json!({
                    "created": false,
                    "policy": policy_json(&policy),
                    "roles": roles_json(&roles),
                    "warnings": ["org policy already exists; init did not reset admins"],
                }),
                Meta::default(),
            ));
        }

        let Some(admin) = args.bootstrap_admin.as_deref() else {
            let mut failure = CommandFailure::new(
                ErrorCode::ArgInvalid,
                "fresh org policy init requires --bootstrap-admin",
            );
            failure.details = json!({
                "manual_bootstrap": "review and create state/registry/roles.json with at least one admin",
            });
            return Err(failure);
        };
        validate_subject(admin)?;
        write_string(&policy_path, &default_policy_toml())?;
        let roles = RolesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            grants: vec![RoleGrantRecord {
                subject: admin.to_string(),
                role: "admin".to_string(),
                granted_at: Utc::now(),
                granted_by: current_actor(),
            }],
        };
        save_roles(&self.ctx, &roles)?;
        let commit = commit_policy_change(
            &self.ctx,
            &paths,
            "policy.org.init",
            json!({"bootstrap_admin": admin, "request_id": request_id}),
            json!({"policy_path": state::ORG_POLICY_REL, "roles_path": state::ROLES_REL}),
            "policy: initialize org policy",
        )?;
        Ok((
            json!({
                "created": true,
                "policy": policy_json(&load_policy_document(&self.ctx)?),
                "roles": roles_json(&load_roles(&self.ctx)?),
                "commit": commit,
            }),
            Meta::default(),
        ))
    }

    fn cmd_policy_org_show(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        let policy = load_policy_document(&self.ctx)?;
        let roles = load_roles(&self.ctx)?;
        let approvals = load_approval_states(&self.ctx)?;
        Ok((
            json!({
                "policy": policy_json(&policy),
                "roles": roles_json(&roles),
                "approvals": approval_summary(&approvals),
                "current_actor": current_actor(),
            }),
            Meta::default(),
        ))
    }

    fn cmd_roles_list(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        load_policy_document(&self.ctx)?;
        let roles = load_roles(&self.ctx)?;
        Ok((
            json!({"roles": roles_json(&roles), "current_actor": current_actor()}),
            Meta::default(),
        ))
    }

    fn cmd_roles_grant(
        &self,
        args: &RoleGrantArgs,
        grant: bool,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        load_policy_document(&self.ctx)?;
        validate_subject(&args.subject)?;
        validate_role(&args.role)?;
        let actor = current_actor();
        let mut roles = load_roles(&self.ctx)?;
        if !subject_has_role(&roles, &actor, "admin") {
            return Err(policy_blocked(
                "roles changes require the current actor to have admin role",
                json!({"actor": actor, "required_roles": ["admin"]}),
            ));
        }
        if grant {
            if !roles
                .grants
                .iter()
                .any(|item| item.subject == args.subject && item.role == args.role)
            {
                roles.grants.push(RoleGrantRecord {
                    subject: args.subject.clone(),
                    role: args.role.clone(),
                    granted_at: Utc::now(),
                    granted_by: actor.clone(),
                });
            }
        } else {
            roles
                .grants
                .retain(|item| !(item.subject == args.subject && item.role == args.role));
            if !has_resolved_admin(&roles) {
                return Err(policy_blocked(
                    "roles revoke would leave org policy without a resolved admin",
                    json!({"subject": args.subject, "role": args.role}),
                ));
            }
        }
        save_roles(&self.ctx, &roles)?;
        let intent = if grant { "roles.grant" } else { "roles.revoke" };
        let commit = commit_policy_change(
            &self.ctx,
            &paths,
            intent,
            json!({"subject": args.subject, "role": args.role, "request_id": request_id}),
            json!({"roles_path": state::ROLES_REL}),
            &format!("policy: {intent}"),
        )?;
        Ok((
            json!({
                "changed": true,
                "action": if grant { "grant" } else { "revoke" },
                "subject": args.subject,
                "role": args.role,
                "roles": roles_json(&roles),
                "commit": commit,
            }),
            Meta::default(),
        ))
    }

    fn cmd_approval_request(
        &self,
        args: &ApprovalRequestArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let check = OrgPolicyCheckArgs {
            action: args.action.clone(),
            skill: args.skill.clone(),
            provider: args.provider.clone(),
            sync_remote: args.sync_remote.clone(),
            agent: args.agent.clone(),
        };
        let decision = evaluate_org_policy(&self.ctx, &check)?;
        if decision.decision == "deny" {
            return Err(policy_blocked(
                "org policy denies this action; approval request not created",
                json!({"policy": decision}),
            ));
        }
        if decision.decision == "allow" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "org policy already allows this action; approval request not required",
            ));
        }
        let request = approval_requested_event(args, &decision);
        append_approval_event(&self.ctx, &request)?;
        let commit = commit_policy_change(
            &self.ctx,
            &paths,
            "approval.request",
            json!({"request_id": request["request_id"], "action": args.action, "request_id_external": request_id}),
            json!({"approvals_path": state::APPROVALS_REL}),
            "policy: request approval",
        )?;
        Ok((
            json!({"request": request, "policy": decision, "commit": commit}),
            Meta::default(),
        ))
    }

    fn cmd_approval_list(
        &self,
        args: &ApprovalListArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        load_policy_document(&self.ctx)?;
        let requests = load_approval_states(&self.ctx)?;
        let filters = [args.pending, args.approved, args.rejected]
            .into_iter()
            .filter(|v| *v)
            .count();
        if filters > 1 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "approval list accepts only one of --pending, --approved, or --rejected",
            ));
        }
        let status = if args.pending {
            Some("pending")
        } else if args.approved {
            Some("approved")
        } else if args.rejected {
            Some("rejected")
        } else {
            None
        };
        let requests = requests
            .into_iter()
            .filter(|request| status.is_none_or(|status| request.status == status))
            .map(approval_state_json)
            .collect::<Vec<_>>();
        Ok((
            json!({"count": requests.len(), "requests": requests}),
            Meta::default(),
        ))
    }

    fn cmd_approval_decision(
        &self,
        args: &ApprovalDecisionArgs,
        approve: bool,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        load_policy_document(&self.ctx)?;
        validate_request_id(&args.request_id)?;
        let request = load_approval_states(&self.ctx)?
            .into_iter()
            .find(|item| item.request_id == args.request_id)
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("approval request '{}' not found", args.request_id),
                )
            })?;
        if request.status != "pending" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "approval request '{}' is already {}",
                    request.request_id, request.status
                ),
            ));
        }
        let actor = current_actor();
        let roles = load_roles(&self.ctx)?;
        let satisfied = request
            .required_roles
            .iter()
            .find(|role| subject_has_role(&roles, &actor, role))
            .cloned()
            .ok_or_else(|| {
                policy_blocked(
                    "current actor does not satisfy any required approval role",
                    json!({"actor": actor, "required_roles": request.required_roles}),
                )
            })?;
        let event = approval_decision_event(args, approve, &actor, &satisfied);
        append_approval_event(&self.ctx, &event)?;
        let intent = if approve {
            "approval.approve"
        } else {
            "approval.reject"
        };
        let commit = commit_policy_change(
            &self.ctx,
            &paths,
            intent,
            json!({"request_id": args.request_id, "actor": actor, "request_id_external": request_id}),
            json!({"approvals_path": state::APPROVALS_REL}),
            &format!("policy: {intent}"),
        )?;
        Ok((
            json!({
                "request_id": args.request_id,
                "status": if approve { "approved" } else { "rejected" },
                "event": event,
                "commit": commit,
            }),
            Meta::default(),
        ))
    }
}

fn evaluate_org_policy(
    ctx: &crate::state::AppContext,
    args: &OrgPolicyCheckArgs,
) -> std::result::Result<state::OrgPolicyDecision, CommandFailure> {
    load_policy_document(ctx)?;
    let action = canonical_action(&args.action)?;
    let subject = subject_for_action(&action, args)?;
    let roles = load_roles(ctx)?;
    let actor = current_actor();
    let actor_roles = roles_for_subject(&roles, &actor);
    let required_roles = required_roles_for_action(&action);
    let mut reasons = Vec::new();
    let mut decision = if required_roles
        .iter()
        .any(|role| subject_has_role(&roles, &actor, role))
    {
        "allow".to_string()
    } else {
        reasons.push(format!(
            "current actor '{}' lacks required role(s): {}",
            actor,
            required_roles.join(", ")
        ));
        "approval_required".to_string()
    };
    let mut evidence = json!({
        "registry_head": gitops::head(ctx).ok(),
        "command_inputs_digest": org_policy_digest_json(&json!({"action": action, "subject": subject})),
    });
    if let Some(skill) = subject.get("skill").and_then(Value::as_str) {
        let trust = trust_metadata_for_skill(ctx, skill)?;
        evidence["skill_trust"] = json!({"trust": trust.trust, "quarantined": trust.quarantined});
        if trust.trust == "blocked" || trust.quarantined {
            decision = "deny".to_string();
            reasons
                .push("blocked or quarantined skill cannot be approved by org policy".to_string());
        }
    }
    let required_approvals = required_roles
        .iter()
        .map(|role| format!("approval:{role}"))
        .collect::<Vec<_>>();
    let approval_request_command = (decision == "approval_required").then(|| {
        let mut command = format!("loom approval request {}", shell_arg(&action));
        if let Some(skill) = subject.get("skill").and_then(Value::as_str) {
            command.push_str(&format!(" --skill {}", shell_arg(skill)));
        }
        if let Some(provider) = subject.get("provider").and_then(Value::as_str) {
            command.push_str(&format!(" --provider {}", shell_arg(provider)));
        }
        if let Some(remote) = subject.get("sync_remote").and_then(Value::as_str) {
            command.push_str(&format!(" --sync-remote {}", shell_arg(remote)));
        }
        if let Some(agent) = subject.get("agent").and_then(Value::as_str) {
            command.push_str(&format!(" --agent {}", shell_arg(agent)));
        }
        command
    });

    Ok(state::OrgPolicyDecision {
        action,
        decision,
        actor,
        actor_roles,
        required_roles,
        required_approvals,
        subject,
        reasons,
        evidence,
        approval_request_command,
    })
}
