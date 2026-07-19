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
    pub projections_digest: Option<String>,
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
        digest_value(&serde_json::to_value(payload)?)
    }
}

const DIGEST_FIELDS: [&str; 13] = [
    "skill",
    "selectors",
    "source",
    "input",
    "preflight",
    "input_conflicts",
    "registry",
    "projections",
    "visibility",
    "accept_restart_required",
    "remote",
    "required_axes",
    "required_approvals",
];

pub(crate) fn stored_plan_digest(plan: &Value) -> Option<Result<String, serde_json::Error>> {
    let plan = plan.as_object()?;
    if !stored_plan_shape_is_valid(plan) {
        return None;
    }
    let mut payload = serde_json::Map::new();
    for field in DIGEST_FIELDS {
        payload.insert(field.to_string(), plan.get(field)?.clone());
    }
    Some(digest_value(&Value::Object(payload)))
}

fn stored_plan_shape_is_valid(plan: &serde_json::Map<String, Value>) -> bool {
    field_is_string(plan, "plan_id")
        && field_is_string(plan, "plan_digest")
        && field_is_string(plan, "skill")
        && field_object_matches(plan, "selectors", selectors_are_valid)
        && field_object_matches(plan, "source", source_is_valid)
        && field_object_matches(plan, "input", input_is_valid)
        && field_object_matches(plan, "preflight", preflight_is_valid)
        && field_object_array_matches(plan, "input_conflicts", input_conflict_is_valid)
        && field_object_matches(plan, "registry", registry_is_valid)
        && field_object_array_matches(plan, "projections", projection_effect_is_valid)
        && field_object_array_matches(plan, "visibility", visibility_is_valid)
        && field_is_bool(plan, "accept_restart_required")
        && field_is_one_of(plan, "remote", &["not_requested", "push"])
        && required_axes_are_valid(plan.get("required_axes"))
        && field_is_string_array(plan, "required_approvals")
}

fn selectors_are_valid(value: &serde_json::Map<String, Value>) -> bool {
    ["agent", "workspace", "profile", "input_instance"]
        .into_iter()
        .all(|field| field_is_optional_string(value, field))
}

fn source_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    field_is_one_of(value, "direction", &["source", "projection"])
        && field_is_string(value, "registry_head")
        && field_is_string(value, "tree_digest")
        && field_is_optional_string(value, "input_instance")
}

fn input_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    field_is_string_array(value, "source_dirty_paths")
        && field_object_array_matches(value, "projections", projection_input_is_valid)
        && field_is_optional_string(value, "selected_projection_instance")
        && field_is_string(value, "selected_input_tree_digest")
}

fn projection_input_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    field_is_string(value, "instance_id")
        && field_is_string(value, "method")
        && field_is_string(value, "materialized_path")
        && field_is_optional_string(value, "baseline_revision")
        && field_is_optional_string(value, "baseline_tree_digest")
        && field_is_optional_string(value, "live_tree_digest")
        && field_is_one_of(
            value,
            "state",
            &[
                "clean",
                "dirty",
                "source_linked",
                "missing",
                "not_directory",
                "unreadable",
                "baseline_unavailable",
                "untracked",
                "metadata_mismatch",
            ],
        )
        && field_is_optional_string(value, "issue")
}

fn preflight_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    field_is_one_of(value, "input_direction", &["source", "projection"])
        && field_is_string(value, "input_tree_digest")
        && value
            .get("checks")
            .and_then(Value::as_object)
            .is_some_and(|checks| checks.values().all(Value::is_string))
        && field_is_string_array(value, "regression_ids")
        && field_is_bool(value, "mutation_allowed")
}

fn input_conflict_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    field_is_string(value, "code")
        && field_is_string(value, "message")
        && value.contains_key("evidence")
}

fn registry_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    field_is_bool(value, "initialized")
        && field_is_optional_string(value, "checkpoint_digest")
        && field_is_optional_string(value, "checkpoint_updated_at")
        && field_is_optional_string(value, "projections_digest")
}

fn projection_effect_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    [
        "instance_id",
        "binding_id",
        "target_id",
        "agent",
        "profile",
        "method",
        "ownership",
        "materialized_path",
        "source_tree_digest",
        "effect",
    ]
    .into_iter()
    .all(|field| field_is_string(value, field))
        && field_is_optional_string(value, "materialized_tree_digest")
}

fn visibility_is_valid(value: &serde_json::Map<String, Value>) -> bool {
    ["agent", "binding_id", "target_id", "check"]
        .into_iter()
        .all(|field| field_is_string(value, field))
        && field_is_bool(value, "required")
}

fn required_axes_are_valid(value: Option<&Value>) -> bool {
    let Some(values) = value.and_then(Value::as_array) else {
        return false;
    };
    let mut previous = None;
    for value in values {
        let rank = match value.as_str() {
            Some("projections") => 0,
            Some("registry_transport") => 1,
            Some("visibility") => 2,
            _ => return false,
        };
        if previous.is_some_and(|previous| previous >= rank) {
            return false;
        }
        previous = Some(rank);
    }
    true
}

fn field_object_matches(
    value: &serde_json::Map<String, Value>,
    field: &str,
    validate: fn(&serde_json::Map<String, Value>) -> bool,
) -> bool {
    value
        .get(field)
        .and_then(Value::as_object)
        .is_some_and(validate)
}

fn field_object_array_matches(
    value: &serde_json::Map<String, Value>,
    field: &str,
    validate: fn(&serde_json::Map<String, Value>) -> bool,
) -> bool {
    value
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .all(|item| item.as_object().is_some_and(validate))
        })
}

fn field_is_string(value: &serde_json::Map<String, Value>, field: &str) -> bool {
    value.get(field).is_some_and(Value::is_string)
}

fn field_is_bool(value: &serde_json::Map<String, Value>, field: &str) -> bool {
    value.get(field).is_some_and(Value::is_boolean)
}

fn field_is_optional_string(value: &serde_json::Map<String, Value>, field: &str) -> bool {
    value
        .get(field)
        .is_some_and(|value| value.is_null() || value.is_string())
}

fn field_is_string_array(value: &serde_json::Map<String, Value>, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().all(Value::is_string))
}

fn field_is_one_of(value: &serde_json::Map<String, Value>, field: &str, allowed: &[&str]) -> bool {
    value
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| allowed.contains(&value))
}

fn digest_value(value: &Value) -> Result<String, serde_json::Error> {
    let bytes = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}
