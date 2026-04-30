#![allow(dead_code)]

mod json_io;
mod persistence;
use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct RegistryStatePaths {
    pub root: PathBuf,
    pub state_dir: PathBuf,
    pub registry_dir: PathBuf,
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
pub struct RegistrySnapshot {
    pub schema: RegistrySchemaFile,
    pub targets: RegistryTargetsFile,
    pub bindings: RegistryBindingsFile,
    pub rules: RegistryRulesFile,
    pub projections: RegistryProjectionsFile,
    pub operations: Vec<RegistryOperationRecord>,
    pub checkpoint: RegistryOpsCheckpoint,
}

#[derive(Debug, Clone)]
pub struct RegistryTargetRelations<'a> {
    pub bindings: Vec<&'a RegistryWorkspaceBinding>,
    pub rules: Vec<&'a RegistryBindingRule>,
    pub projections: Vec<&'a RegistryProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySchemaFile {
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub writer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTargetsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub targets: Vec<RegistryProjectionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBindingsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub bindings: Vec<RegistryWorkspaceBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryRulesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<RegistryBindingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryProjectionsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub projections: Vec<RegistryProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryTarget"
)]
pub struct RegistryProjectionTarget {
    pub target_id: String,
    pub agent: String,
    pub path: String,
    pub ownership: String,
    pub capabilities: RegistryTargetCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/")]
pub struct RegistryTargetCapabilities {
    pub symlink: bool,
    pub copy: bool,
    pub watch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryBinding"
)]
pub struct RegistryWorkspaceBinding {
    pub binding_id: String,
    pub agent: String,
    pub profile_id: String,
    pub workspace_matcher: RegistryWorkspaceMatcher,
    pub default_target_id: String,
    pub policy_profile: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/")]
pub struct RegistryWorkspaceMatcher {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "RegistryRule")]
pub struct RegistryBindingRule {
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
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryProjection"
)]
pub struct RegistryProjectionInstance {
    pub instance_id: String,
    pub skill_id: String,
    // `Some(id)` means the projection is owned by that binding; `None` means
    // the projection is orphaned after its binding was removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub binding_id: Option<String>,
    pub target_id: String,
    pub materialized_path: String,
    pub method: String,
    pub last_applied_rev: String,
    // Valid values: "healthy", "drifted", "missing", "conflict", "orphaned".
    pub health: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub observed_drift: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryOperationRecord {
    pub op_id: String,
    pub intent: String,
    pub status: String,
    pub ack: bool,
    pub payload: serde_json::Value,
    pub effects: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<RegistryOperationError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryOperationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryCheckpoint"
)]
pub struct RegistryOpsCheckpoint {
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
pub struct RegistryObservationEvent {
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

pub(crate) fn empty_targets_file() -> RegistryTargetsFile {
    RegistryTargetsFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        targets: Vec::new(),
    }
}

pub(crate) fn empty_bindings_file() -> RegistryBindingsFile {
    RegistryBindingsFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        bindings: Vec::new(),
    }
}

pub(crate) fn empty_rules_file() -> RegistryRulesFile {
    RegistryRulesFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        rules: Vec::new(),
    }
}

pub(crate) fn empty_projections_file() -> RegistryProjectionsFile {
    RegistryProjectionsFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        projections: Vec::new(),
    }
}

use std::path::PathBuf;

impl RegistrySnapshot {
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

    pub fn binding(&self, binding_id: &str) -> Option<&RegistryWorkspaceBinding> {
        self.bindings
            .bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
    }

    pub fn target(&self, target_id: &str) -> Option<&RegistryProjectionTarget> {
        self.targets
            .targets
            .iter()
            .find(|target| target.target_id == target_id)
    }

    pub fn binding_default_target(
        &self,
        binding: &RegistryWorkspaceBinding,
    ) -> Option<RegistryProjectionTarget> {
        self.target(&binding.default_target_id).cloned()
    }

    pub fn binding_rules(&self, binding_id: &str) -> Vec<RegistryBindingRule> {
        self.rules
            .rules
            .iter()
            .filter(|rule| rule.binding_id == binding_id)
            .cloned()
            .collect()
    }

    pub fn binding_projections(&self, binding_id: &str) -> Vec<RegistryProjectionInstance> {
        self.projections
            .projections
            .iter()
            .filter(|projection| projection.binding_id.as_deref() == Some(binding_id))
            .cloned()
            .collect()
    }

    pub fn target_rules(&self, target_id: &str) -> Vec<RegistryBindingRule> {
        self.target_relations(target_id)
            .rules
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn target_projections(&self, target_id: &str) -> Vec<RegistryProjectionInstance> {
        self.target_relations(target_id)
            .projections
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn target_bindings(&self, target_id: &str) -> Vec<RegistryWorkspaceBinding> {
        self.target_relations(target_id)
            .bindings
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn target_relations(&self, target_id: &str) -> RegistryTargetRelations<'_> {
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
                .filter_map(|projection| projection.binding_id.as_deref()),
        );

        let mut bindings = Vec::with_capacity(self.bindings.bindings.len());
        for binding in &self.bindings.bindings {
            if binding.default_target_id == target_id
                || linked_binding_ids.contains(binding.binding_id.as_str())
            {
                bindings.push(binding);
            }
        }

        RegistryTargetRelations {
            bindings,
            rules,
            projections,
        }
    }
}
