use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::ActivationScope;
use crate::state_model::{
    RegistryProjectionInstance, RegistryProjectionTarget, RegistryWorkspaceBinding,
};

use super::super::helpers::projection_method_as_str;
use super::resolve::{ActivationResolved, scope_str};

#[derive(Debug, Serialize)]
pub(super) struct ActivationPlan {
    skill: String,
    agent: String,
    scope: &'static str,
    profile: String,
    workspace: Option<String>,
    target_id: String,
    target_path: String,
    binding_id: String,
    materialized_path: String,
    method: String,
    actions: Vec<ActivationAction>,
    noop: bool,
    dry_run: bool,
    visibility_claim: &'static str,
}

#[derive(Debug, Serialize)]
struct ActivationAction {
    action: &'static str,
    status: &'static str,
    path: Option<String>,
}

pub(super) fn activation_plan(resolved: &ActivationResolved, dry_run: bool) -> ActivationPlan {
    let mut actions = Vec::new();
    action(
        &mut actions,
        resolved.target_is_new,
        "create_target",
        Some(&resolved.target.path),
    );
    action(
        &mut actions,
        resolved.binding_is_new,
        "create_binding",
        None,
    );
    action(
        &mut actions,
        resolved
            .existing_rule
            .as_ref()
            .is_none_or(|rule| rule.method != projection_method_as_str(resolved.selection.method)),
        "upsert_rule",
        None,
    );
    action(
        &mut actions,
        resolved
            .existing_projection
            .as_ref()
            .is_none_or(|projection| {
                projection.method != projection_method_as_str(resolved.selection.method)
                    || projection.materialized_path
                        != resolved.materialized_path.display().to_string()
                    || projection.health != "healthy"
            })
            || !projection_exists_for_plan(resolved),
        "project_skill",
        Some(&resolved.materialized_path.display().to_string()),
    );
    let noop = actions
        .iter()
        .all(|action| action.status == "already_satisfied");
    ActivationPlan {
        skill: resolved.selection.skill.clone(),
        agent: resolved.selection.agent.clone(),
        scope: scope_str(resolved.selection.scope),
        profile: resolved.selection.profile.clone(),
        workspace: resolved
            .selection
            .workspace
            .as_ref()
            .map(|path| path.display().to_string()),
        target_id: resolved.target.target_id.clone(),
        target_path: resolved.target.path.clone(),
        binding_id: resolved.binding.binding_id.clone(),
        materialized_path: resolved.materialized_path.display().to_string(),
        method: projection_method_as_str(resolved.selection.method).to_string(),
        actions,
        noop,
        dry_run,
        visibility_claim: "not_checked",
    }
}

pub(super) fn deactivation_plan(resolved: Option<&ActivationResolved>, dry_run: bool) -> Value {
    let Some(resolved) = resolved else {
        return json!({
            "actions": [],
            "noop": true,
            "dry_run": dry_run,
            "visibility_claim": "not_checked"
        });
    };
    json!({
        "skill": resolved.selection.skill,
        "agent": resolved.selection.agent,
        "scope": scope_str(resolved.selection.scope),
        "profile": resolved.selection.profile,
        "target_id": resolved.target.target_id,
        "binding_id": resolved.binding.binding_id,
        "materialized_path": resolved.materialized_path.display().to_string(),
        "actions": [
            {
                "action": "remove_rule",
                "status": if resolved.existing_rule.is_some() { "will_apply" } else { "already_satisfied" },
                "path": null
            },
            {
                "action": "remove_safe_symlink_projection",
                "status": if resolved.existing_projection.is_some() { "will_apply" } else { "already_satisfied" },
                "path": resolved.materialized_path.display().to_string()
            }
        ],
        "noop": resolved.existing_rule.is_none() && resolved.existing_projection.is_none(),
        "dry_run": dry_run,
        "visibility_claim": "not_checked"
    })
}

pub(super) fn activation_state_changed(resolved: &ActivationResolved) -> bool {
    resolved.target_is_new
        || resolved.binding_is_new
        || resolved
            .existing_rule
            .as_ref()
            .is_none_or(|rule| rule.method != projection_method_as_str(resolved.selection.method))
        || resolved
            .existing_projection
            .as_ref()
            .is_none_or(|projection| {
                projection.method != projection_method_as_str(resolved.selection.method)
                    || projection.materialized_path
                        != resolved.materialized_path.display().to_string()
                    || projection.health != "healthy"
            })
}

pub(super) fn active_status(
    source_exists: bool,
    target: Option<&RegistryProjectionTarget>,
    projection: Option<&RegistryProjectionInstance>,
) -> String {
    if !source_exists {
        return "source_missing".to_string();
    }
    let Some(target) = target else {
        return "target_missing".to_string();
    };
    if !Path::new(&target.path).exists() {
        return "target_missing".to_string();
    }
    let Some(projection) = projection else {
        return "missing_projection".to_string();
    };
    let path = Path::new(&projection.materialized_path);
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        projection.health.clone()
    } else {
        "missing_projection".to_string()
    }
}

pub(super) fn binding_matches_scope(
    binding: &RegistryWorkspaceBinding,
    scope: ActivationScope,
    workspace: Option<&Path>,
) -> bool {
    match scope {
        ActivationScope::User => {
            binding.workspace_matcher.kind == "name" && binding.workspace_matcher.value == "user"
        }
        ActivationScope::Project => {
            let Some(workspace) = workspace else {
                return false;
            };
            match binding.workspace_matcher.kind.as_str() {
                "path_prefix" => workspace.starts_with(Path::new(&binding.workspace_matcher.value)),
                "exact_path" => workspace == Path::new(&binding.workspace_matcher.value),
                _ => false,
            }
        }
    }
}

fn action(
    actions: &mut Vec<ActivationAction>,
    needed: bool,
    name: &'static str,
    path: Option<&str>,
) {
    actions.push(ActivationAction {
        action: name,
        status: if needed {
            "will_apply"
        } else {
            "already_satisfied"
        },
        path: path.map(ToString::to_string),
    });
}

fn projection_exists_for_plan(resolved: &ActivationResolved) -> bool {
    resolved.materialized_path.exists() || fs::symlink_metadata(&resolved.materialized_path).is_ok()
}
