use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::ActivationScope;
use crate::state_model::{
    RegistryProjectionInstance, RegistryProjectionTarget, RegistryWorkspaceBinding,
};

use super::super::CommandFailure;
use super::super::helpers::{map_io, projection_method_as_str};
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
                    || projection.health != crate::core::vocab::Health::Healthy
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
                    || projection.health != crate::core::vocab::Health::Healthy
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
        projection.health.to_string()
    } else {
        "missing_projection".to_string()
    }
}

pub(super) fn binding_matches_scope(
    binding: &RegistryWorkspaceBinding,
    scope: ActivationScope,
    workspace: Option<&Path>,
) -> std::result::Result<bool, CommandFailure> {
    match scope {
        // User bindings use a scope marker. It is intentionally not interpreted
        // as the final component of a project workspace.
        ActivationScope::User => {
            Ok(binding.workspace_matcher.kind == "name"
                && binding.workspace_matcher.value == "user")
        }
        ActivationScope::Project => {
            let Some(workspace) = workspace else {
                return Ok(false);
            };
            match binding.workspace_matcher.kind.as_str() {
                "path_prefix" | "exact_path" => binding
                    .workspace_matcher
                    .matches_workspace(workspace)
                    .map_err(map_io),
                // A project selector never consumes the user-scope marker.
                "name" => Ok(false),
                _ => Ok(false),
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

#[cfg(all(test, unix))]
mod workspace_matcher_tests {
    use super::binding_matches_scope;
    use crate::cli::ActivationScope;
    use crate::core::vocab::MatcherKind;
    use crate::state_model::{RegistryWorkspaceBinding, RegistryWorkspaceMatcher};
    use crate::types::ErrorCode;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    fn binding(kind: MatcherKind, value: &Path) -> RegistryWorkspaceBinding {
        RegistryWorkspaceBinding {
            binding_id: "binding-test".to_string(),
            agent: "codex".into(),
            profile_id: "default".to_string(),
            workspace_matcher: RegistryWorkspaceMatcher {
                kind,
                value: value.display().to_string(),
            },
            default_target_id: "target-test".to_string(),
            policy_profile: "safe-capture".to_string(),
            active: true,
            created_at: None,
        }
    }

    #[test]
    fn project_scope_uses_authoritative_symlink_matching() {
        let base = std::env::temp_dir().join(format!(
            "loom_activation_match_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let real = base.join("real");
        std::fs::create_dir_all(real.join("project")).expect("create project");
        let link = base.join("link");
        symlink(&real, &link).expect("create workspace symlink");
        let binding = binding(MatcherKind::PathPrefix, &real);

        assert!(
            binding_matches_scope(
                &binding,
                ActivationScope::Project,
                Some(&link.join("project")),
            )
            .expect("project matcher")
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn user_scope_marker_is_not_a_project_basename_matcher() {
        let binding = binding(MatcherKind::Name, Path::new("user"));
        assert!(
            binding_matches_scope(&binding, ActivationScope::User, None)
                .expect("user scope marker")
        );
        assert!(
            !binding_matches_scope(
                &binding,
                ActivationScope::Project,
                Some(Path::new("/workspace/user")),
            )
            .expect("project scope")
        );
    }

    #[test]
    fn project_scope_maps_symlink_loop_to_io_error() {
        let base = std::env::temp_dir().join(format!(
            "loom_activation_loop_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&base).expect("create loop root");
        let left = base.join("left");
        let right = base.join("right");
        symlink(&right, &left).expect("create first loop edge");
        symlink(&left, &right).expect("create second loop edge");
        let binding = binding(MatcherKind::ExactPath, &left);

        let err = binding_matches_scope(&binding, ActivationScope::Project, Some(&left))
            .expect_err("loop must reach command error");
        assert_eq!(err.code, ErrorCode::IoError);

        std::fs::remove_file(&left).ok();
        std::fs::remove_file(&right).ok();
        std::fs::remove_dir_all(&base).ok();
    }
}
