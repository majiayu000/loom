use serde_json::{Value, json};

use crate::cli::OrgPolicyCheckArgs;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::validate_skill_name;
use super::state::validate_subject;

#[derive(Debug, Clone, Default)]
pub(crate) struct PolicyCheck {
    pub action: String,
    pub skill: Option<String>,
    pub provider: Option<String>,
    pub sync_remote: Option<String>,
    pub agent: Option<String>,
    pub trash_id: Option<String>,
    pub target_id: Option<String>,
    pub binding_id: Option<String>,
    pub skillset: Option<String>,
}

impl PolicyCheck {
    pub(crate) fn new(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            ..Self::default()
        }
    }

    pub(crate) fn from_args(args: &OrgPolicyCheckArgs) -> Self {
        Self {
            action: args.action.clone(),
            skill: args.skill.clone(),
            provider: args.provider.clone(),
            sync_remote: args.sync_remote.clone(),
            agent: args.agent.clone(),
            trash_id: None,
            target_id: None,
            binding_id: None,
            skillset: None,
        }
    }

    pub(crate) fn skill(mut self, value: impl Into<String>) -> Self {
        self.skill = Some(value.into());
        self
    }

    pub(crate) fn provider(mut self, value: impl Into<String>) -> Self {
        self.provider = Some(value.into());
        self
    }

    pub(crate) fn sync_remote(mut self, value: impl Into<String>) -> Self {
        self.sync_remote = Some(value.into());
        self
    }

    pub(crate) fn agent(mut self, value: impl Into<String>) -> Self {
        self.agent = Some(value.into());
        self
    }

    pub(crate) fn trash_id(mut self, value: impl Into<String>) -> Self {
        self.trash_id = Some(value.into());
        self
    }

    pub(crate) fn target_id(mut self, value: impl Into<String>) -> Self {
        self.target_id = Some(value.into());
        self
    }

    pub(crate) fn binding_id(mut self, value: impl Into<String>) -> Self {
        self.binding_id = Some(value.into());
        self
    }

    pub(crate) fn skillset(mut self, value: impl Into<String>) -> Self {
        self.skillset = Some(value.into());
        self
    }
}

pub(super) fn canonical_action(action: &str) -> std::result::Result<String, CommandFailure> {
    let normalized = match action {
        "workspace.remote" => "workspace.remote.set",
        "skill.new" => "skill.author.new",
        "skill.trust" => "skill.trust.update",
        "skill.unquarantine" => "skill.quarantine",
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
        "skill.author.new"
        | "skill.author.apply_patch"
        | "skill.save"
        | "skill.capture"
        | "skill.watch"
        | "skill.add"
        | "skill.install"
        | "skill.import_observed"
        | "skill.monitor_observed"
        | "skill.trash.add"
        | "skill.trash.restore" => "author",
        "skill.activate"
        | "skill.deactivate"
        | "skill.project"
        | "skillset.activate"
        | "skillset.deactivate" => "reviewer",
        "skill.release"
        | "skill.snapshot"
        | "skill.rollback"
        | "skill.trust.update"
        | "skill.trust"
        | "skill.quarantine"
        | "skill.provenance.refresh"
        | "skill.trash.purge"
        | "skill.orphan.clean"
        | "skillset.release"
        | "skillset.rollback"
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

fn action_requires_skill(action: &str) -> bool {
    action.starts_with("skill.")
        && !matches!(
            action,
            "skill.orphan.clean"
                | "skill.import_observed"
                | "skill.monitor_observed"
                | "skill.watch"
                | "skill.capture"
                | "skill.trash.purge"
                | "skill.author.apply_patch"
        )
}

pub(super) fn subject_for_action(
    action: &str,
    args: &PolicyCheck,
) -> std::result::Result<Value, CommandFailure> {
    let mut subject = serde_json::Map::new();
    if action_requires_skill(action) {
        let Some(skill) = args.skill.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("action '{action}' requires --skill"),
            ));
        };
        insert_skill_subject(&mut subject, skill)?;
    } else if let Some(skill) = args.skill.as_deref() {
        insert_skill_subject(&mut subject, skill)?;
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
    if action == "skill.trash.purge" {
        let Some(trash_id) = args.trash_id.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "action 'skill.trash.purge' requires a trash id",
            ));
        };
        validate_subject(trash_id)?;
        subject.insert("trash_id".to_string(), json!(trash_id));
    }
    if action == "target.remove" {
        let Some(target_id) = args.target_id.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "action 'target.remove' requires a target id",
            ));
        };
        validate_subject(target_id)?;
        subject.insert("target_id".to_string(), json!(target_id));
    } else if let Some(target_id) = args.target_id.as_deref() {
        validate_subject(target_id)?;
        subject.insert("target_id".to_string(), json!(target_id));
    }
    if action == "workspace.binding.remove" {
        let Some(binding_id) = args.binding_id.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "action 'workspace.binding.remove' requires a binding id",
            ));
        };
        validate_subject(binding_id)?;
        subject.insert("binding_id".to_string(), json!(binding_id));
    } else if let Some(binding_id) = args.binding_id.as_deref() {
        validate_subject(binding_id)?;
        subject.insert("binding_id".to_string(), json!(binding_id));
    }
    if action.starts_with("skillset.") {
        let Some(skillset) = args.skillset.as_deref() else {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("action '{action}' requires a skillset"),
            ));
        };
        validate_subject(skillset)?;
        subject.insert("skillset".to_string(), json!(skillset));
    }
    if let Some(agent) = args.agent.as_deref() {
        validate_subject(agent)?;
        subject.insert("agent".to_string(), json!(agent));
    }
    Ok(Value::Object(subject))
}

fn insert_skill_subject(
    subject: &mut serde_json::Map<String, Value>,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(|err| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("invalid skill subject: {err}"),
        )
    })?;
    subject.insert("skill".to_string(), json!(skill));
    Ok(())
}

pub(super) fn identity_subject(subject: &Value) -> Value {
    const KEYS: &[&str] = &[
        "skill",
        "provider",
        "sync_remote",
        "trash_id",
        "target_id",
        "binding_id",
        "skillset",
    ];
    let mut identity = serde_json::Map::new();
    if let Some(object) = subject.as_object() {
        for key in KEYS {
            if let Some(value) = object.get(*key) {
                identity.insert((*key).to_string(), value.clone());
            }
        }
    }
    Value::Object(identity)
}
