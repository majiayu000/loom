use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::state_model::REGISTRY_SCHEMA_VERSION;

pub(super) const WORKFLOWS_REL: &str = "state/registry/workflows.json";
pub(super) const WORKFLOW_PLANS_REL: &str = "state/registry/workflow_plans.json";
pub(super) const WORKFLOW_PLAN_SCHEMA: &str = "workflow-plan-v1";
pub(super) const PLAN_PROTOCOL_VERSION: &str = "1.0";
pub(super) const DEFAULT_MAX_NODES: usize = 32;
pub(super) const DEFAULT_MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowsFile {
    pub schema_version: u32,
    pub workflows: Vec<WorkflowRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowPlansFile {
    pub schema_version: u32,
    pub plans: Vec<StoredWorkflowPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowRecord {
    pub workflow_id: String,
    pub description: String,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub external_inputs: Vec<String>,
    #[serde(default)]
    pub policy: WorkflowPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowInput {
    #[serde(default, alias = "id")]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default)]
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub external_inputs: Vec<String>,
    #[serde(default)]
    pub policy: WorkflowPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowNode {
    pub id: String,
    pub skill_id: String,
    #[serde(default = "default_node_kind")]
    pub kind: String,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub mutates_workspace: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorkflowPolicy {
    #[serde(default)]
    pub max_nodes: Option<usize>,
    #[serde(default)]
    pub max_depth: Option<usize>,
    #[serde(default)]
    pub requires_human_approval_before: Vec<String>,
    #[serde(default = "default_approval_for_mutations")]
    pub approval_required_for_mutations: bool,
    #[serde(default)]
    pub rollback_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct StoredWorkflowPlan {
    pub plan_id: String,
    pub schema_version: String,
    pub protocol_version: String,
    pub workflow_id: String,
    pub workflow_digest: String,
    pub registry_root: String,
    pub registry_head: String,
    pub agent: String,
    pub workspace: String,
    pub skill_digests: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub ready: bool,
    pub payload: Value,
}

impl Default for WorkflowPolicy {
    fn default() -> Self {
        Self {
            max_nodes: Some(DEFAULT_MAX_NODES),
            max_depth: Some(DEFAULT_MAX_DEPTH),
            requires_human_approval_before: Vec::new(),
            approval_required_for_mutations: true,
            rollback_strategy: Some("checkpoint-before-mutating-node".to_string()),
        }
    }
}

impl WorkflowsFile {
    pub fn empty() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            workflows: Vec::new(),
        }
    }

    pub fn normalize(&mut self) {
        self.workflows
            .sort_by(|left, right| left.workflow_id.cmp(&right.workflow_id));
        for workflow in &mut self.workflows {
            workflow.nodes.sort_by(|left, right| left.id.cmp(&right.id));
            workflow.edges.sort_by(|left, right| {
                left.from
                    .cmp(&right.from)
                    .then_with(|| left.to.cmp(&right.to))
            });
            workflow.external_inputs.sort();
            workflow.external_inputs.dedup();
            workflow.policy.requires_human_approval_before.sort();
            workflow.policy.requires_human_approval_before.dedup();
        }
    }

    pub fn find(&self, workflow_id: &str) -> Option<&WorkflowRecord> {
        self.workflows
            .iter()
            .find(|workflow| workflow.workflow_id == workflow_id)
    }
}

impl WorkflowPlansFile {
    pub fn empty() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            plans: Vec::new(),
        }
    }

    pub fn normalize(&mut self) {
        self.plans.sort_by_key(|plan| plan.created_at);
        if self.plans.len() > 200 {
            self.plans.drain(0..self.plans.len() - 200);
        }
    }

    pub fn find(&self, plan_id: &str) -> Option<&StoredWorkflowPlan> {
        self.plans.iter().find(|plan| plan.plan_id == plan_id)
    }
}

fn default_node_kind() -> String {
    "skill".to_string()
}

fn default_approval_for_mutations() -> bool {
    true
}
