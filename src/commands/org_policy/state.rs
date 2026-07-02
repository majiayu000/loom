use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use toml_edit::DocumentMut;
use uuid::Uuid;

use crate::cli::{ApprovalDecisionArgs, ApprovalRequestArgs, OrgPolicyCheckArgs};
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::{REGISTRY_SCHEMA_VERSION, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::helpers::{map_git, map_io, map_registry_state, validate_skill_name};
use super::super::projections::record_registry_operation;
use super::super::{CommandFailure, redact_sensitive_string};

pub(super) const ORG_POLICY_REL: &str = "state/registry/org_policy.toml";
pub(super) const ROLES_REL: &str = "state/registry/roles.json";
pub(super) const APPROVALS_REL: &str = "state/registry/approvals.jsonl";
const POLICY_SCHEMA: &str = "loom.policy.v1";
const VALID_ROLES: &[&str] = &["viewer", "author", "reviewer", "maintainer", "admin"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RolesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub grants: Vec<RoleGrantRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RoleGrantRecord {
    pub subject: String,
    pub role: String,
    pub granted_at: DateTime<Utc>,
    pub granted_by: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct OrgPolicyDecision {
    pub action: String,
    pub decision: String,
    pub actor: String,
    pub actor_roles: Vec<String>,
    pub required_roles: Vec<String>,
    pub required_approvals: Vec<String>,
    pub subject: Value,
    pub reasons: Vec<String>,
    pub evidence: Value,
    pub approval_request_command: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ApprovalRequestState {
    pub request_id: String,
    pub action: String,
    pub subject: Value,
    pub requester: String,
    pub reason_redacted: Option<String>,
    pub required_roles: Vec<String>,
    pub required_approvals: Vec<String>,
    pub policy_decision_digest: String,
    pub evidence: Value,
    pub created_at: String,
    pub status: String,
    pub decisions: Vec<Value>,
}

pub(super) fn org_policy_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(ORG_POLICY_REL)
}

pub(super) fn roles_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(ROLES_REL)
}

fn approvals_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(APPROVALS_REL)
}

pub(super) fn default_policy_toml() -> String {
    r#"schema = "loom.policy.v1"

[requirements."skill.activate"]
blocked = "deny"
quarantined = "deny"
required_role = "reviewer"

[requirements."skill.release"]
required_role = "maintainer"
requires_clean_source = true
requires_eval_pass = true
requires_security_scan = true
requires_reviewer_approval = true

[requirements."provider.add"]
required_role = "maintainer"

[requirements."provider.remove"]
required_role = "maintainer"

[requirements."roles.grant"]
required_role = "admin"

[requirements."roles.revoke"]
required_role = "admin"
"#
    .to_string()
}

pub(super) fn load_policy_document(
    ctx: &AppContext,
) -> std::result::Result<DocumentMut, CommandFailure> {
    let path = org_policy_path(ctx);
    if !path.exists() {
        return Err(CommandFailure::new(
            ErrorCode::StateNotInitialized,
            "org policy is not initialized; run loom policy org init --bootstrap-admin <user>",
        ));
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let doc = raw.parse::<DocumentMut>().map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {ORG_POLICY_REL}: {err}"),
        )
    })?;
    if doc["schema"].as_str() != Some(POLICY_SCHEMA) {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("{ORG_POLICY_REL} schema must be {POLICY_SCHEMA}"),
        ));
    }
    Ok(doc)
}

pub(super) fn policy_json(doc: &DocumentMut) -> Value {
    let requirements = doc["requirements"]
        .as_table()
        .map(|table| {
            table
                .iter()
                .map(|(action, item)| {
                    let mut requirement = serde_json::Map::new();
                    if let Some(table) = item.as_table() {
                        for (key, value) in table.iter() {
                            requirement.insert(key.to_string(), toml_value_json(value));
                        }
                    }
                    (action.to_string(), Value::Object(requirement))
                })
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    json!({
        "schema": doc["schema"].as_str().unwrap_or(""),
        "path": ORG_POLICY_REL,
        "requirements": requirements,
    })
}

fn toml_value_json(value: &toml_edit::Item) -> Value {
    if let Some(value) = value.as_value() {
        if let Some(v) = value.as_str() {
            return json!(v);
        }
        if let Some(v) = value.as_bool() {
            return json!(v);
        }
        if let Some(v) = value.as_integer() {
            return json!(v);
        }
    }
    Value::Null
}

pub(super) fn load_roles(ctx: &AppContext) -> std::result::Result<RolesFile, CommandFailure> {
    let path = roles_path(ctx);
    if !path.exists() {
        return Ok(RolesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            grants: Vec::new(),
        });
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let roles: RolesFile = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {ROLES_REL}: {err}"),
        )
    })?;
    if roles.schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("{ROLES_REL} schema version mismatch"),
        ));
    }
    for grant in &roles.grants {
        validate_subject(&grant.subject)?;
        validate_role(&grant.role)?;
    }
    Ok(roles)
}

pub(super) fn save_roles(
    ctx: &AppContext,
    roles: &RolesFile,
) -> std::result::Result<(), CommandFailure> {
    let mut roles = roles.clone();
    roles.grants.sort_by(|a, b| {
        a.subject
            .cmp(&b.subject)
            .then_with(|| a.role.cmp(&b.role))
            .then_with(|| a.granted_at.cmp(&b.granted_at))
    });
    let mut seen = BTreeSet::new();
    roles
        .grants
        .retain(|grant| seen.insert((grant.subject.clone(), grant.role.clone())));
    let raw = serde_json::to_string_pretty(&roles).map_err(map_io)? + "\n";
    write_string(&roles_path(ctx), &raw)
}

pub(super) fn roles_json(roles: &RolesFile) -> Value {
    let mut by_subject: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for grant in &roles.grants {
        by_subject
            .entry(grant.subject.clone())
            .or_default()
            .push(grant.role.clone());
    }
    for roles in by_subject.values_mut() {
        roles.sort_by_key(|role| role_rank(role).unwrap_or(0));
        roles.dedup();
    }
    json!({
        "path": ROLES_REL,
        "grants": roles.grants,
        "by_subject": by_subject,
        "unresolved_teams": roles.grants.iter().filter(|grant| grant.subject.starts_with("team:")).map(|grant| grant.subject.clone()).collect::<BTreeSet<_>>(),
    })
}

pub(super) fn append_approval_event(
    ctx: &AppContext,
    event: &Value,
) -> std::result::Result<(), CommandFailure> {
    let path = approvals_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    let raw = serde_json::to_string(event).map_err(map_io)? + "\n";
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(map_io)?;
    file.write_all(raw.as_bytes()).map_err(map_io)?;
    file.sync_all().map_err(map_io)?;
    Ok(())
}

fn load_approval_events(ctx: &AppContext) -> std::result::Result<Vec<Value>, CommandFailure> {
    let path = approvals_path(ctx);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let mut events = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).map_err(|err| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("failed to parse {APPROVALS_REL} line {}: {err}", index + 1),
            )
        })?;
        if value["event"].as_str().is_none() || value["request_id"].as_str().is_none() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!(
                    "{APPROVALS_REL} line {} is missing event or request_id",
                    index + 1
                ),
            ));
        }
        events.push(value);
    }
    Ok(events)
}

pub(super) fn load_approval_states(
    ctx: &AppContext,
) -> std::result::Result<Vec<ApprovalRequestState>, CommandFailure> {
    let mut states = BTreeMap::<String, ApprovalRequestState>::new();
    for event in load_approval_events(ctx)? {
        let request_id = event["request_id"].as_str().unwrap_or_default().to_string();
        match event["event"].as_str().unwrap_or_default() {
            "requested" => {
                states.insert(
                    request_id.clone(),
                    ApprovalRequestState {
                        request_id,
                        action: event["action"].as_str().unwrap_or_default().to_string(),
                        subject: event["subject"].clone(),
                        requester: event["requester"].as_str().unwrap_or_default().to_string(),
                        reason_redacted: event["reason_redacted"].as_str().map(str::to_string),
                        required_roles: string_array(&event["required_roles"]),
                        required_approvals: string_array(&event["required_approvals"]),
                        policy_decision_digest: event["policy_decision_digest"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        evidence: event["evidence"].clone(),
                        created_at: event["created_at"].as_str().unwrap_or_default().to_string(),
                        status: "pending".to_string(),
                        decisions: Vec::new(),
                    },
                );
            }
            "approved" | "rejected" => {
                let Some(state) = states.get_mut(&request_id) else {
                    return Err(CommandFailure::new(
                        ErrorCode::StateCorrupt,
                        format!("approval decision references unknown request '{request_id}'"),
                    ));
                };
                if state.status != "pending" {
                    return Err(CommandFailure::new(
                        ErrorCode::StateCorrupt,
                        format!("approval request '{request_id}' has multiple terminal decisions"),
                    ));
                }
                state.status = event["event"].as_str().unwrap_or_default().to_string();
                state.decisions.push(event);
            }
            other => {
                return Err(CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    format!("unknown approval event '{other}'"),
                ));
            }
        }
    }
    Ok(states.into_values().collect())
}

pub(super) fn approval_summary(requests: &[ApprovalRequestState]) -> Value {
    let mut pending = 0;
    let mut approved = 0;
    let mut rejected = 0;
    for request in requests {
        match request.status.as_str() {
            "pending" => pending += 1,
            "approved" => approved += 1,
            "rejected" => rejected += 1,
            _ => {}
        }
    }
    json!({"path": APPROVALS_REL, "pending": pending, "approved": approved, "rejected": rejected})
}

pub(super) fn approval_state_json(state: ApprovalRequestState) -> Value {
    json!({
        "request_id": state.request_id,
        "action": state.action,
        "subject": state.subject,
        "requester": state.requester,
        "reason_redacted": state.reason_redacted,
        "required_roles": state.required_roles,
        "required_approvals": state.required_approvals,
        "policy_decision_digest": state.policy_decision_digest,
        "evidence": state.evidence,
        "created_at": state.created_at,
        "status": state.status,
        "decisions": state.decisions,
    })
}

pub(super) fn approval_requested_event(
    args: &ApprovalRequestArgs,
    decision: &OrgPolicyDecision,
) -> Value {
    json!({
        "event": "requested",
        "request_id": format!("approval_{}", Uuid::new_v4().simple()),
        "action": decision.action,
        "subject": decision.subject,
        "requester": current_actor(),
        "reason_redacted": args.reason.as_deref().map(redact_comment),
        "risk_summary": {"policy_reasons": decision.reasons.len()},
        "evidence": decision.evidence,
        "required_roles": decision.required_roles,
        "required_approvals": decision.required_approvals,
        "policy_decision_digest": org_policy_digest_json(&json!(decision)),
        "created_at": Utc::now().to_rfc3339(),
    })
}

pub(super) fn approval_decision_event(
    args: &ApprovalDecisionArgs,
    approve: bool,
    actor: &str,
    satisfied_role: &str,
) -> Value {
    json!({
        "event": if approve { "approved" } else { "rejected" },
        "request_id": args.request_id,
        "actor": actor,
        "satisfied_approval": format!("approval:{satisfied_role}"),
        "comment_redacted": args.comment.as_deref().map(redact_comment),
        "created_at": Utc::now().to_rfc3339(),
    })
}

pub(super) fn current_actor() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown-local-actor".to_string())
}

pub(super) fn roles_for_subject(roles: &RolesFile, subject: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    for grant in &roles.grants {
        if grant.subject == subject {
            found.insert(grant.role.clone());
        }
    }
    found.into_iter().collect()
}

pub(super) fn subject_has_role(roles: &RolesFile, subject: &str, required: &str) -> bool {
    let Some(required_rank) = role_rank(required) else {
        return false;
    };
    roles.grants.iter().any(|grant| {
        grant.subject == subject && role_rank(&grant.role).is_some_and(|rank| rank >= required_rank)
    })
}

pub(super) fn role_rank(role: &str) -> Option<u8> {
    match role {
        "viewer" => Some(1),
        "author" => Some(2),
        "reviewer" => Some(3),
        "maintainer" => Some(4),
        "admin" => Some(5),
        _ => None,
    }
}

pub(super) fn validate_role(role: &str) -> std::result::Result<(), CommandFailure> {
    if VALID_ROLES.contains(&role) {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("role must be one of {}", VALID_ROLES.join(", ")),
        ))
    }
}

pub(super) fn validate_subject(subject: &str) -> std::result::Result<(), CommandFailure> {
    if subject.is_empty() || subject.len() > 128 {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "role subject must be 1-128 characters",
        ));
    }
    if subject
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@' | ':' | '/')))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "role subject contains unsupported characters",
        ));
    }
    Ok(())
}

pub(super) fn validate_request_id(value: &str) -> std::result::Result<(), CommandFailure> {
    if value.starts_with("approval_")
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "approval request id must look like approval_<id>",
        ))
    }
}

pub(super) fn has_resolved_admin(roles: &RolesFile) -> bool {
    roles
        .grants
        .iter()
        .any(|grant| grant.role == "admin" && !grant.subject.starts_with("team:"))
}

pub(super) fn canonical_action(action: &str) -> std::result::Result<String, CommandFailure> {
    let normalized = match action {
        "workspace.remote" => "workspace.remote.set",
        other => other,
    };
    if required_roles_for_action(normalized).is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsupported org policy action '{action}'"),
        ));
    }
    Ok(normalized.to_string())
}

pub(super) fn required_roles_for_action(action: &str) -> Vec<String> {
    let role = match action {
        "skill.new"
        | "skill.save"
        | "skill.capture"
        | "skill.watch"
        | "skill.snapshot"
        | "skill.add"
        | "skill.install"
        | "skill.import_observed"
        | "skill.monitor_observed"
        | "skill.trash.add"
        | "skill.trash.restore" => "author",
        "skill.activate" | "skill.deactivate" | "skill.project" => "reviewer",
        "skill.release"
        | "skill.rollback"
        | "skill.trust.update"
        | "skill.trust"
        | "skill.quarantine"
        | "skill.provenance.refresh"
        | "skill.trash.purge"
        | "skill.orphan.clean"
        | "provider.add"
        | "provider.remove"
        | "target.add"
        | "target.remove"
        | "workspace.remote.set"
        | "workspace.binding.add"
        | "workspace.binding.remove"
        | "sync.pull"
        | "sync.push"
        | "sync.replay"
        | "ops.retry"
        | "ops.purge"
        | "ops.history.repair" => "maintainer",
        "roles.grant" | "roles.revoke" | "policy.org.init" => "admin",
        _ => return Vec::new(),
    };
    vec![role.to_string()]
}

pub(super) fn subject_for_action(
    action: &str,
    args: &OrgPolicyCheckArgs,
) -> std::result::Result<Value, CommandFailure> {
    let mut subject = serde_json::Map::new();
    if action.starts_with("skill.") {
        let Some(skill) = args.skill.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("action '{action}' requires --skill"),
            ));
        };
        validate_skill_name(skill).map_err(|err| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("invalid skill subject: {err}"),
            )
        })?;
        subject.insert("skill".to_string(), json!(skill));
    }
    if action.starts_with("provider.") {
        let Some(provider) = args.provider.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("action '{action}' requires --provider"),
            ));
        };
        validate_subject(provider)?;
        subject.insert("provider".to_string(), json!(provider));
    }
    if action.starts_with("sync.") {
        let Some(remote) = args.sync_remote.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("action '{action}' requires --sync-remote"),
            ));
        };
        validate_subject(remote)?;
        subject.insert("sync_remote".to_string(), json!(remote));
    }
    if let Some(agent) = args.agent.as_deref() {
        validate_subject(agent)?;
        subject.insert("agent".to_string(), json!(agent));
    }
    Ok(Value::Object(subject))
}

pub(super) fn org_policy_digest_json(value: &Value) -> String {
    let raw = serde_json::to_vec(value).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&raw);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

pub(super) fn write_string(path: &Path, contents: &str) -> std::result::Result<(), CommandFailure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    fs::write(path, contents).map_err(map_io)
}

pub(super) fn commit_policy_change(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    intent: &str,
    input: Value,
    effects: Value,
    message: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    record_registry_operation(paths, intent, input, effects).map_err(map_registry_state)?;
    gitops::commit_paths_if_changed(ctx, &["state/registry", ".gitignore"], message)
        .map_err(map_git)
}

pub(super) fn policy_blocked(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}

pub(super) fn shell_arg(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn string_array(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn redact_comment(value: &str) -> String {
    redact_sensitive_string(value)
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.starts_with("token=")
                || lower.starts_with("password=")
                || lower.starts_with("secret=")
                || lower.starts_with("api_key=")
            {
                let key = part.split_once('=').map(|(key, _)| key).unwrap_or("secret");
                format!("{key}=[REDACTED]")
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
