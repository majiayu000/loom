use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

use crate::state::AppContext;
use crate::state_model::{RegistryStatePaths, RegistryWorkspaceBinding};
use crate::types::ErrorCode;

use super::helpers::map_registry_state;
use super::{CommandFailure, build_skill_read_model};

pub(crate) type ActivationPlanDelta = (Vec<Value>, Vec<Value>, Vec<Value>, Vec<String>);

#[derive(Default)]
pub(crate) struct ActiveView {
    binding_id: Option<String>,
    workspace: Option<String>,
    active_skills: BTreeSet<String>,
}

pub(crate) fn active_view(
    ctx: &AppContext,
    agent: &str,
    workspace: Option<&Path>,
    binding_id: Option<&str>,
) -> std::result::Result<ActiveView, CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let Some(snapshot) = paths.maybe_load_snapshot().map_err(map_registry_state)? else {
        return Ok(ActiveView::default());
    };
    let binding = if let Some(binding_id) = binding_id {
        let binding = snapshot
            .bindings
            .bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
            .ok_or_else(|| {
                CommandFailure::new(ErrorCode::ArgInvalid, format!("no binding '{binding_id}'"))
            })?;
        if binding.agent != agent || !binding.active {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("binding '{binding_id}' inactive for '{agent}'"),
            ));
        }
        Some(binding)
    } else {
        let candidates = snapshot
            .bindings
            .bindings
            .iter()
            .filter(|binding| {
                binding.agent == agent
                    && binding.active
                    && recommend_binding_matches_workspace(binding, workspace)
            })
            .collect::<Vec<_>>();
        if candidates.len() > 1 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "multiple active bindings; pass --binding",
            ));
        }
        candidates.into_iter().next()
    };
    let Some(binding) = binding else {
        return Ok(ActiveView::default());
    };
    let active_skills = snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.binding_id == binding.binding_id)
        .map(|rule| rule.skill_id.clone())
        .collect::<BTreeSet<_>>();
    Ok(ActiveView {
        binding_id: Some(binding.binding_id.clone()),
        workspace: command_workspace(workspace, binding),
        active_skills,
    })
}

pub(crate) fn activation_plan_delta(
    ctx: &AppContext,
    desired: &[String],
    agent: &str,
    workspace: Option<&Path>,
    active_view: &ActiveView,
) -> std::result::Result<ActivationPlanDelta, CommandFailure> {
    let inventory = build_skill_read_model(ctx)
        .map_err(map_registry_state)?
        .skills
        .into_iter()
        .filter_map(|skill| {
            skill["skill_id"]
                .as_str()
                .map(|id| (id.to_string(), skill.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut add = Vec::new();
    let mut keep = Vec::new();
    let mut risks = Vec::new();
    let desired_set = desired.iter().cloned().collect::<BTreeSet<_>>();
    for skill in desired {
        if active_view.active_skills.contains(skill) {
            keep.push(json!({
                "skill": skill,
                "action": "keep",
                "status": "already_active",
                "binding_id": active_view.binding_id,
            }));
            continue;
        }
        match inventory.get(skill) {
            Some(record) if record["quarantined"].as_bool() == Some(true) => {
                risks.push(format!("'{skill}' quarantined"));
            }
            Some(record) if record["trust"].as_str() == Some("blocked") => {
                risks.push(format!("'{skill}' trust blocked"));
            }
            Some(record) if record["source_status"].as_str() != Some("present") => {
                risks.push(format!(
                    "'{skill}' source {}",
                    record["source_status"].as_str().unwrap_or("unknown")
                ));
            }
            Some(record) if !record["warnings"].as_array().is_none_or(Vec::is_empty) => {
                risks.push(format!("'{skill}' inventory warnings"));
            }
            Some(_) => add.push(json!({
                "skill": skill,
                "action": "activate",
                "status": "planned",
                "command": activation_command(
                    "activate",
                    skill,
                    agent,
                    workspace.or_else(|| active_view.workspace.as_deref().map(Path::new)),
                ),
            })),
            None => risks.push(format!("'{skill}' missing")),
        }
    }
    let remove = active_view
        .active_skills
        .iter()
        .filter(|skill| !desired_set.contains(*skill))
        .map(|skill| {
            json!({
                "skill": skill,
                "action": "deactivate",
                "status": "planned",
                "binding_id": active_view.binding_id,
                "command": activation_command(
                    "deactivate",
                    skill,
                    agent,
                    workspace.or_else(|| active_view.workspace.as_deref().map(Path::new)),
                ),
            })
        })
        .collect::<Vec<_>>();
    Ok((add, keep, remove, risks))
}

fn recommend_binding_matches_workspace(
    binding: &RegistryWorkspaceBinding,
    workspace: Option<&Path>,
) -> bool {
    match workspace {
        None => true,
        Some(workspace) => binding.workspace_matcher.matches_workspace(workspace),
    }
}

fn command_workspace(
    workspace: Option<&Path>,
    binding: &RegistryWorkspaceBinding,
) -> Option<String> {
    workspace
        .map(|path| path.display().to_string())
        .or_else(|| match binding.workspace_matcher.kind.as_str() {
            "path_prefix" | "exact_path" => Some(binding.workspace_matcher.value.clone()),
            _ => None,
        })
}

fn activation_command(action: &str, skill: &str, agent: &str, workspace: Option<&Path>) -> String {
    match workspace {
        Some(workspace) => format!(
            "loom --json skill {action} {skill} --agent {agent} --scope project --workspace {} --dry-run",
            workspace.display()
        ),
        None => format!("loom --json skill {action} {skill} --agent {agent} --dry-run"),
    }
}
