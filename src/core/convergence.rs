use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectionInputState {
    Clean,
    Dirty,
    SourceLinked,
    Missing,
    NotDirectory,
    Unreadable,
    BaselineUnavailable,
    Untracked,
    MetadataMismatch,
}

impl ProjectionInputState {
    pub(crate) fn is_dirty(&self) -> bool {
        matches!(self, Self::Dirty)
    }

    pub(crate) fn is_usable_input(&self) -> bool {
        matches!(self, Self::Clean | Self::Dirty)
    }

    pub(crate) fn is_fail_closed(&self) -> bool {
        matches!(
            self,
            Self::Unreadable
                | Self::BaselineUnavailable
                | Self::NotDirectory
                | Self::MetadataMismatch
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProjectionInputEvidence {
    pub instance_id: String,
    pub method: String,
    pub materialized_path: String,
    pub baseline_revision: Option<String>,
    pub baseline_tree_digest: Option<String>,
    pub live_tree_digest: Option<String>,
    pub state: ProjectionInputState,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ConvergenceInputEvidence {
    pub source_dirty_paths: Vec<String>,
    pub projections: Vec<ProjectionInputEvidence>,
    pub selected_projection_instance: Option<String>,
    pub selected_input_tree_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ConvergencePreflightEvidence {
    pub input_direction: ConvergenceInputDirection,
    pub input_tree_digest: String,
    pub checks: BTreeMap<String, String>,
    pub regression_ids: Vec<String>,
    pub mutation_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ConvergenceInputConflict {
    pub code: String,
    pub message: String,
    pub evidence: Value,
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
    pub input: ConvergenceInputEvidence,
    pub preflight: ConvergencePreflightEvidence,
    pub input_conflicts: Vec<ConvergenceInputConflict>,
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
    input: &'a ConvergenceInputEvidence,
    preflight: &'a ConvergencePreflightEvidence,
    input_conflicts: &'a [ConvergenceInputConflict],
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
        self.plan_digest = self.canonical_digest()?;
        Ok(())
    }

    pub(crate) fn canonical_digest(&self) -> Result<String, serde_json::Error> {
        let payload = SkillConvergenceDigestPayload {
            skill: &self.skill,
            selectors: &self.selectors,
            source: &self.source,
            input: &self.input,
            preflight: &self.preflight,
            input_conflicts: &self.input_conflicts,
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
        Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
    }
}
