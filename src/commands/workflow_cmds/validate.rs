use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::super::CommandFailure;
use super::super::helpers::{map_arg, validate_skill_name};
use super::model::{DEFAULT_MAX_DEPTH, DEFAULT_MAX_NODES, WorkflowNode, WorkflowRecord};
use crate::types::ErrorCode;

pub(super) fn validate_workflow_definition(
    workflow: &WorkflowRecord,
) -> std::result::Result<Vec<String>, CommandFailure> {
    if workflow.nodes.is_empty() {
        return Err(validation_error(
            "WORKFLOW_EMPTY",
            "workflow must contain at least one node",
        ));
    }
    let max_nodes = workflow.policy.max_nodes.unwrap_or(DEFAULT_MAX_NODES);
    if workflow.nodes.len() > max_nodes {
        return Err(validation_error(
            "WORKFLOW_TOO_LARGE",
            format!(
                "workflow has {} nodes but policy allows {}",
                workflow.nodes.len(),
                max_nodes
            ),
        ));
    }

    let mut ids = BTreeSet::new();
    for node in &workflow.nodes {
        validate_node_id(&node.id)?;
        validate_skill_name(&node.skill_id).map_err(map_arg)?;
        if !ids.insert(node.id.as_str()) {
            return Err(validation_error(
                "NODE_DUPLICATE",
                format!("duplicate workflow node '{}'", node.id),
            ));
        }
        if node.kind != "skill" {
            return Err(validation_error(
                "NODE_KIND_UNSUPPORTED",
                format!(
                    "workflow node '{}' has unsupported kind '{}'",
                    node.id, node.kind
                ),
            ));
        }
    }
    for approval_node in &workflow.policy.requires_human_approval_before {
        if !ids.contains(approval_node.as_str()) {
            return Err(validation_error(
                "APPROVAL_NODE_MISSING",
                format!("approval policy references unknown node '{approval_node}'"),
            ));
        }
    }
    for edge in &workflow.edges {
        if !ids.contains(edge.from.as_str()) {
            return Err(validation_error(
                "EDGE_NODE_MISSING",
                format!("edge references unknown from node '{}'", edge.from),
            ));
        }
        if !ids.contains(edge.to.as_str()) {
            return Err(validation_error(
                "EDGE_NODE_MISSING",
                format!("edge references unknown to node '{}'", edge.to),
            ));
        }
        if edge.from == edge.to {
            return Err(validation_error(
                "SELF_EDGE",
                format!("node '{}' cannot depend on itself", edge.from),
            ));
        }
    }

    let order = topological_order(workflow)?;
    let max_depth = workflow.policy.max_depth.unwrap_or(DEFAULT_MAX_DEPTH);
    if workflow_depth(workflow, &order) > max_depth {
        return Err(validation_error(
            "WORKFLOW_TOO_DEEP",
            format!("workflow depth exceeds policy max_depth={max_depth}"),
        ));
    }
    validate_required_outputs(workflow, &order)?;
    Ok(order)
}

pub(super) fn workflow_node<'a>(
    workflow: &'a WorkflowRecord,
    node_id: &str,
) -> std::result::Result<&'a WorkflowNode, CommandFailure> {
    workflow
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| {
            validation_error(
                "NODE_MISSING",
                format!("workflow node '{node_id}' disappeared during validation"),
            )
        })
}

pub(super) fn validate_workflow_id(value: &str) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(value).map_err(map_arg)
}

pub(super) fn validate_node_id(value: &str) -> std::result::Result<(), CommandFailure> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
    {
        return Err(validation_error(
            "NODE_ID_INVALID",
            format!("node id '{}' must match [A-Za-z0-9_-]", value),
        ));
    }
    Ok(())
}

pub(super) fn validate_plan_id(value: &str) -> std::result::Result<(), CommandFailure> {
    if !value.starts_with("workflow_plan_") {
        return Err(validation_error(
            "PLAN_ID_INVALID",
            "workflow plan id must start with workflow_plan_",
        ));
    }
    validate_node_id(value)
}

pub(super) fn validation_error(code: &str, message: impl Into<String>) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::ArgInvalid, message);
    failure.details = json!({ "validation_code": code });
    failure
}

fn topological_order(
    workflow: &WorkflowRecord,
) -> std::result::Result<Vec<String>, CommandFailure> {
    let mut indegree = workflow
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut children: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for edge in &workflow.edges {
        *indegree.entry(edge.to.as_str()).or_insert(0) += 1;
        children
            .entry(edge.from.as_str())
            .or_default()
            .insert(edge.to.as_str());
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(*node))
        .collect::<BTreeSet<_>>();
    let mut out = Vec::with_capacity(workflow.nodes.len());
    while let Some(node) = ready.pop_first() {
        out.push(node.to_string());
        if let Some(next) = children.get(node) {
            for child in next {
                let child_id = *child;
                let count = indegree.get_mut(child_id).ok_or_else(|| {
                    validation_error(
                        "EDGE_NODE_MISSING",
                        format!("edge references unknown node '{child_id}'"),
                    )
                })?;
                *count -= 1;
                if *count == 0 {
                    ready.insert(child_id);
                }
            }
        }
    }
    if out.len() != workflow.nodes.len() {
        return Err(validation_error(
            "CYCLE_DETECTED",
            "workflow graph contains a cycle",
        ));
    }
    Ok(out)
}

fn workflow_depth(workflow: &WorkflowRecord, order: &[String]) -> usize {
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in &workflow.edges {
        children
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }
    let mut depth = workflow
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), 1usize))
        .collect::<BTreeMap<_, _>>();
    for node in order {
        let current = *depth.get(node.as_str()).unwrap_or(&1);
        if let Some(next) = children.get(node.as_str()) {
            for child in next {
                let entry = depth.entry(*child).or_insert(1);
                *entry = (*entry).max(current + 1);
            }
        }
    }
    depth.values().copied().max().unwrap_or(0)
}

fn validate_required_outputs(
    workflow: &WorkflowRecord,
    order: &[String],
) -> std::result::Result<(), CommandFailure> {
    let mut available = workflow
        .external_inputs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for node_id in order {
        let node = workflow_node(workflow, node_id)?;
        for required in &node.requires {
            if !available.contains(required.as_str()) {
                return Err(validation_error(
                    "REQUIRED_INPUT_MISSING",
                    format!(
                        "node '{}' requires '{}' before any upstream node outputs it",
                        node.id, required
                    ),
                ));
            }
        }
        available.extend(node.outputs.iter().map(String::as_str));
    }
    Ok(())
}
