use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

pub(super) const PROVISION_PLAN_SCHEMA: &str = "provision-plan-v1";

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProvisionPlan {
    pub schema_version: String,
    pub plan_id: String,
    pub created_at: DateTime<Utc>,
    pub target_kind: String,
    pub workspace: String,
    pub container_workspace: String,
    pub agents: Vec<String>,
    pub registry_source_display: String,
    pub registry_clone_url: Option<String>,
    pub active_views: Vec<ProvisionActiveView>,
    pub dependency_readiness: Vec<ProvisionDependencyReadiness>,
    pub files_to_write: Vec<ProvisionFilePlan>,
    pub secrets_required: Vec<ProvisionSecretRequirement>,
    pub policy: Value,
    pub loom_cli: Value,
    pub guards: Value,
    pub findings: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProvisionActiveView {
    pub agent: String,
    pub scope: String,
    pub path: String,
    pub binding_id: Option<String>,
    pub source_target_id: Option<String>,
    pub source_target_path: Option<String>,
    pub skills: Vec<String>,
    pub skillsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProvisionDependencyReadiness {
    pub skill: String,
    pub status: String,
    pub ready: bool,
    pub next_actions: Vec<String>,
    pub findings: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProvisionFilePlan {
    pub path: String,
    pub kind: String,
    pub safe_to_apply: bool,
    pub preimage_digest: Option<String>,
    pub content_digest: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ProvisionSecretRequirement {
    pub name: String,
    pub required: bool,
    pub present: bool,
    pub redacted: bool,
    pub source: String,
}
