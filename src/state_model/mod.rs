#![allow(dead_code)]

mod persistence;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

pub const V3_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone)]
pub struct V3StatePaths {
    pub root: PathBuf,
    pub state_dir: PathBuf,
    pub v3_dir: PathBuf,
    pub schema_file: PathBuf,
    pub targets_file: PathBuf,
    pub bindings_file: PathBuf,
    pub rules_file: PathBuf,
    pub projections_file: PathBuf,
    pub ops_dir: PathBuf,
    pub operations_file: PathBuf,
    pub checkpoint_file: PathBuf,
    pub observations_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Snapshot {
    pub schema: V3SchemaFile,
    pub targets: V3TargetsFile,
    pub bindings: V3BindingsFile,
    pub rules: V3RulesFile,
    pub projections: V3ProjectionsFile,
    pub operations: Vec<V3OperationRecord>,
    pub checkpoint: V3OpsCheckpoint,
}

#[derive(Debug, Clone)]
pub struct V3TargetRelations<'a> {
    pub bindings: Vec<&'a V3WorkspaceBinding>,
    pub rules: Vec<&'a V3BindingRule>,
    pub projections: Vec<&'a V3ProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3SchemaFile {
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub writer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3TargetsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub targets: Vec<V3ProjectionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3BindingsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub bindings: Vec<V3WorkspaceBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3RulesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<V3BindingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ProjectionsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub projections: Vec<V3ProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "V3Target")]
pub struct V3ProjectionTarget {
    pub target_id: String,
    pub agent: String,
    pub path: String,
    pub ownership: String,
    pub capabilities: V3TargetCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/")]
pub struct V3TargetCapabilities {
    pub symlink: bool,
    pub copy: bool,
    pub watch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "V3Binding")]
pub struct V3WorkspaceBinding {
    pub binding_id: String,
    pub agent: String,
    pub profile_id: String,
    pub workspace_matcher: V3WorkspaceMatcher,
    pub default_target_id: String,
    pub policy_profile: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/")]
pub struct V3WorkspaceMatcher {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "V3Rule")]
pub struct V3BindingRule {
    pub binding_id: String,
    pub skill_id: String,
    pub target_id: String,
    pub method: String,
    pub watch_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "V3Projection")]
pub struct V3ProjectionInstance {
    pub instance_id: String,
    pub skill_id: String,
    pub binding_id: String,
    pub target_id: String,
    pub materialized_path: String,
    pub method: String,
    pub last_applied_rev: String,
    pub health: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub observed_drift: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3OperationRecord {
    pub op_id: String,
    pub intent: String,
    pub status: String,
    pub ack: bool,
    pub payload: serde_json::Value,
    pub effects: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<V3OperationError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3OperationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "V3Checkpoint")]
pub struct V3OpsCheckpoint {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_scanned_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_acked_op_id: Option<String>,
    #[ts(type = "string")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ObservationEvent {
    pub event_id: String,
    pub instance_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub observed_at: DateTime<Utc>,
}

pub(crate) fn empty_targets_file() -> V3TargetsFile {
    V3TargetsFile {
        schema_version: V3_SCHEMA_VERSION,
        targets: Vec::new(),
    }
}

pub(crate) fn empty_bindings_file() -> V3BindingsFile {
    V3BindingsFile {
        schema_version: V3_SCHEMA_VERSION,
        bindings: Vec::new(),
    }
}

pub(crate) fn empty_rules_file() -> V3RulesFile {
    V3RulesFile {
        schema_version: V3_SCHEMA_VERSION,
        rules: Vec::new(),
    }
}

pub(crate) fn empty_projections_file() -> V3ProjectionsFile {
    V3ProjectionsFile {
        schema_version: V3_SCHEMA_VERSION,
        projections: Vec::new(),
    }
}

use std::path::PathBuf;

impl V3Snapshot {
    pub fn status_view(&self) -> serde_json::Value {
        let mut unique_skills =
            HashSet::with_capacity(self.rules.rules.len() + self.projections.projections.len());
        for rule in &self.rules.rules {
            unique_skills.insert(rule.skill_id.as_str());
        }

        let mut drifted = 0;
        for projection in &self.projections.projections {
            unique_skills.insert(projection.skill_id.as_str());
            if projection.observed_drift.unwrap_or(false) || projection.health != "healthy" {
                drifted += 1;
            }
        }

        let active_bindings = self
            .bindings
            .bindings
            .iter()
            .filter(|binding| binding.active)
            .count();

        json!({
            "schema_version": self.schema.schema_version,
            "counts": {
                "skills": unique_skills.len(),
                "targets": self.targets.targets.len(),
                "bindings": self.bindings.bindings.len(),
                "active_bindings": active_bindings,
                "rules": self.rules.rules.len(),
                "projections": self.projections.projections.len(),
                "drifted_projections": drifted,
                "operations": self.operations.len()
            },
            "targets": self.targets.targets,
            "bindings": self.bindings.bindings,
            "rules": self.rules.rules,
            "projections": self.projections.projections,
            "checkpoint": self.checkpoint
        })
    }

    pub fn binding(&self, binding_id: &str) -> Option<&V3WorkspaceBinding> {
        self.bindings
            .bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
    }

    pub fn target(&self, target_id: &str) -> Option<&V3ProjectionTarget> {
        self.targets
            .targets
            .iter()
            .find(|target| target.target_id == target_id)
    }

    pub fn binding_default_target(
        &self,
        binding: &V3WorkspaceBinding,
    ) -> Option<V3ProjectionTarget> {
        self.target(&binding.default_target_id).cloned()
    }

    pub fn binding_rules(&self, binding_id: &str) -> Vec<V3BindingRule> {
        self.rules
            .rules
            .iter()
            .filter(|rule| rule.binding_id == binding_id)
            .cloned()
            .collect()
    }

    pub fn binding_projections(&self, binding_id: &str) -> Vec<V3ProjectionInstance> {
        self.projections
            .projections
            .iter()
            .filter(|projection| projection.binding_id == binding_id)
            .cloned()
            .collect()
    }

    pub fn target_rules(&self, target_id: &str) -> Vec<V3BindingRule> {
        self.target_relations(target_id)
            .rules
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn target_projections(&self, target_id: &str) -> Vec<V3ProjectionInstance> {
        self.target_relations(target_id)
            .projections
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn target_bindings(&self, target_id: &str) -> Vec<V3WorkspaceBinding> {
        self.target_relations(target_id)
            .bindings
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn target_relations(&self, target_id: &str) -> V3TargetRelations<'_> {
        let mut rules = Vec::with_capacity(self.rules.rules.len());
        for rule in &self.rules.rules {
            if rule.target_id == target_id {
                rules.push(rule);
            }
        }

        let mut projections = Vec::with_capacity(self.projections.projections.len());
        for projection in &self.projections.projections {
            if projection.target_id == target_id {
                projections.push(projection);
            }
        }

        let mut linked_binding_ids = HashSet::with_capacity(rules.len() + projections.len());
        linked_binding_ids.extend(rules.iter().map(|rule| rule.binding_id.as_str()));
        linked_binding_ids.extend(
            projections
                .iter()
                .map(|projection| projection.binding_id.as_str()),
        );

        let mut bindings = Vec::with_capacity(self.bindings.bindings.len());
        for binding in &self.bindings.bindings {
            if binding.default_target_id == target_id
                || linked_binding_ids.contains(binding.binding_id.as_str())
            {
                bindings.push(binding);
            }
        }

        V3TargetRelations {
            bindings,
            rules,
            projections,
        }
    }
}
