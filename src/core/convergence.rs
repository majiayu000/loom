use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::sha256::{Sha256, to_hex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConvergenceInputDirection {
    Source,
    Projection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConvergenceAxis {
    Projections,
    RegistryTransport,
    Visibility,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ConvergenceSelectors {
    pub agent: Option<String>,
    pub workspace: Option<String>,
    pub profile: Option<String>,
    pub input_instance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SourceGuard {
    pub direction: ConvergenceInputDirection,
    pub registry_head: String,
    pub tree_digest: String,
    pub input_instance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RegistryGuard {
    pub initialized: bool,
    pub checkpoint_digest: Option<String>,
    pub checkpoint_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProjectionEffectPlan {
    pub instance_id: String,
    pub binding_id: String,
    pub target_id: String,
    pub agent: String,
    pub profile: String,
    pub method: String,
    pub ownership: String,
    pub materialized_path: String,
    pub source_tree_digest: String,
    pub materialized_tree_digest: Option<String>,
    pub effect: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct VisibilityRequirement {
    pub agent: String,
    pub binding_id: String,
    pub target_id: String,
    pub check: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RemotePolicy {
    NotRequested,
    Push,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillConvergencePlan {
    pub plan_id: String,
    pub plan_digest: String,
    pub skill: String,
    pub selectors: ConvergenceSelectors,
    pub source: SourceGuard,
    pub registry: RegistryGuard,
    pub projections: Vec<ProjectionEffectPlan>,
    pub visibility: Vec<VisibilityRequirement>,
    pub accept_restart_required: bool,
    pub remote: RemotePolicy,
    pub required_axes: BTreeSet<ConvergenceAxis>,
    pub required_approvals: Vec<String>,
}

#[derive(Serialize)]
struct SkillConvergenceDigestPayload<'a> {
    skill: &'a str,
    selectors: &'a ConvergenceSelectors,
    source: &'a SourceGuard,
    registry: &'a RegistryGuard,
    projections: &'a [ProjectionEffectPlan],
    visibility: &'a [VisibilityRequirement],
    accept_restart_required: bool,
    remote: &'a RemotePolicy,
    required_axes: &'a BTreeSet<ConvergenceAxis>,
    required_approvals: &'a [String],
}

impl SkillConvergencePlan {
    pub(crate) fn seal(&mut self) -> Result<(), serde_json::Error> {
        let payload = SkillConvergenceDigestPayload {
            skill: &self.skill,
            selectors: &self.selectors,
            source: &self.source,
            registry: &self.registry,
            projections: &self.projections,
            visibility: &self.visibility,
            accept_restart_required: self.accept_restart_required,
            remote: &self.remote,
            required_axes: &self.required_axes,
            required_approvals: &self.required_approvals,
        };
        let bytes = serde_json::to_vec(&payload)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        self.plan_digest = format!("sha256:{}", to_hex(&hasher.finalize()));
        Ok(())
    }
}
