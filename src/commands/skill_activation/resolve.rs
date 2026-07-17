use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::agent_adapters::{
    SOURCE_BUILT_IN, built_in_projection_root, load_agent_adapters, preferred_discovery_root,
};
use crate::cli::{ActivationScope, ProjectionMethod, TargetOwnership};
use crate::state::resolve_agent_skill_dirs;
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryBindingRule, RegistryOpsCheckpoint,
    RegistryProjectionInstance, RegistryProjectionTarget, RegistrySnapshot, RegistryStatePaths,
    RegistryWorkspaceBinding, RegistryWorkspaceMatcher, empty_bindings_file,
    empty_projections_file, empty_rules_file, empty_targets_file,
};
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{
    map_arg, map_io, map_registry_state, slugify, target_capabilities, target_ownership_as_str,
    unique_target_id_for_agent, validate_projection_method, validate_skill_name,
};

const DEFAULT_PROFILE: &str = "default";
pub(super) const DEFAULT_POLICY_PROFILE: &str = "safe-capture";

#[derive(Debug, Clone)]
pub(super) struct ActivationSelection {
    pub(super) skill: String,
    pub(super) agent: String,
    pub(super) scope: ActivationScope,
    pub(super) profile: String,
    pub(super) workspace: Option<PathBuf>,
    pub(super) target_id: Option<String>,
    pub(super) method: ProjectionMethod,
}

#[derive(Debug, Clone)]
pub(super) struct ActivationResolved {
    pub(super) selection: ActivationSelection,
    pub(super) target: RegistryProjectionTarget,
    pub(super) target_is_new: bool,
    pub(super) binding: RegistryWorkspaceBinding,
    pub(super) binding_is_new: bool,
    pub(super) materialized_path: PathBuf,
    pub(super) existing_rule: Option<RegistryBindingRule>,
    pub(super) existing_projection: Option<RegistryProjectionInstance>,
}

pub(super) fn activation_selection(
    skill: &str,
    agent: &str,
    scope: ActivationScope,
    workspace: Option<PathBuf>,
    profile: Option<String>,
    target_id: Option<String>,
    method: ProjectionMethod,
) -> std::result::Result<ActivationSelection, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let agent = normalize_agent(agent)?;
    let workspace = workspace_for_scope(scope, workspace)?;
    let profile = profile.unwrap_or_else(|| DEFAULT_PROFILE.to_string());
    if profile.trim().is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--profile must not be empty",
        ));
    }
    if target_id.as_deref().is_some_and(str::is_empty) {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--target must not be empty",
        ));
    }
    Ok(ActivationSelection {
        skill: skill.to_string(),
        agent,
        scope,
        profile,
        workspace,
        target_id,
        method,
    })
}

pub(super) fn resolve_activation(
    ctx: &crate::state::AppContext,
    snapshot: &RegistrySnapshot,
    selection: ActivationSelection,
) -> std::result::Result<ActivationResolved, CommandFailure> {
    let (target, target_is_new) = resolve_target(ctx, snapshot, &selection, true)?;
    if target.agent != selection.agent {
        return Err(CommandFailure::new(
            ErrorCode::TargetAgentMismatch,
            format!(
                "target '{}' is for agent '{}' but activation requested '{}'",
                target.target_id, target.agent, selection.agent
            ),
        ));
    }
    if target.ownership != target_ownership_as_str(TargetOwnership::Managed) {
        return Err(CommandFailure::new(
            ErrorCode::TargetNotManaged,
            format!(
                "target '{}' has ownership '{}' and cannot be activated into",
                target.target_id, target.ownership
            ),
        ));
    }
    validate_projection_method(&target, selection.method)?;
    let (binding, binding_is_new) = resolve_binding(snapshot, &selection, &target);
    let materialized_path = PathBuf::from(&target.path).join(&selection.skill);
    let existing_rule = find_rule(snapshot, &binding, &target, &selection.skill).cloned();
    let existing_projection =
        find_projection(snapshot, &binding, &target, &selection.skill).cloned();
    Ok(ActivationResolved {
        selection,
        target,
        target_is_new,
        binding,
        binding_is_new,
        materialized_path,
        existing_rule,
        existing_projection,
    })
}

pub(super) fn resolve_deactivation(
    ctx: &crate::state::AppContext,
    snapshot: &RegistrySnapshot,
    selection: ActivationSelection,
) -> std::result::Result<Option<ActivationResolved>, CommandFailure> {
    let (target, _) = match resolve_target(ctx, snapshot, &selection, false) {
        Ok(resolved) => resolved,
        Err(err) if matches!(err.code, ErrorCode::TargetNotFound) => return Ok(None),
        Err(err) => return Err(err),
    };
    let Some(binding) = find_matching_binding(snapshot, &selection, &target).cloned() else {
        return Ok(None);
    };
    let materialized_path = PathBuf::from(&target.path).join(&selection.skill);
    let existing_rule = find_rule(snapshot, &binding, &target, &selection.skill).cloned();
    let existing_projection =
        find_projection(snapshot, &binding, &target, &selection.skill).cloned();
    if existing_rule.is_none() && existing_projection.is_none() {
        return Ok(None);
    }
    Ok(Some(ActivationResolved {
        selection,
        target,
        target_is_new: false,
        binding,
        binding_is_new: false,
        materialized_path,
        existing_rule,
        existing_projection,
    }))
}

fn resolve_target(
    ctx: &crate::state::AppContext,
    snapshot: &RegistrySnapshot,
    selection: &ActivationSelection,
    create_missing: bool,
) -> std::result::Result<(RegistryProjectionTarget, bool), CommandFailure> {
    if let Some(target_id) = selection.target_id.as_deref() {
        let target = snapshot.target(target_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", target_id),
            )
        })?;
        return Ok((target, false));
    }
    let path = default_target_path(ctx, selection)?;
    let normalized = normalize_existing_or_raw(&path);
    if let Some(existing) = snapshot.targets.targets.iter().find(|target| {
        target.agent == selection.agent && target_path_matches(&target.path, &normalized)
    }) {
        return Ok((existing.clone(), false));
    }
    if !create_missing {
        return Err(CommandFailure::new(
            ErrorCode::TargetNotFound,
            format!(
                "no managed target registered for agent '{}' at '{}'",
                selection.agent,
                normalized.display()
            ),
        ));
    }
    let normalized_path = normalized.to_string_lossy().into_owned();
    Ok((
        RegistryProjectionTarget {
            target_id: unique_target_id_for_agent(
                &selection.agent,
                &normalized_path,
                &snapshot.targets,
            ),
            agent: selection.agent.clone().into(),
            path: normalized_path,
            ownership: crate::core::vocab::Ownership::Managed,
            capabilities: target_capabilities(TargetOwnership::Managed),
            created_at: Some(Utc::now()),
        },
        true,
    ))
}

fn resolve_binding(
    snapshot: &RegistrySnapshot,
    selection: &ActivationSelection,
    target: &RegistryProjectionTarget,
) -> (RegistryWorkspaceBinding, bool) {
    if let Some(existing) = find_matching_binding(snapshot, selection, target) {
        return (existing.clone(), false);
    }
    let matcher = workspace_matcher(selection);
    (
        RegistryWorkspaceBinding {
            binding_id: unique_activation_binding_id(&snapshot.bindings, selection, &matcher),
            agent: selection.agent.clone().into(),
            profile_id: selection.profile.clone(),
            workspace_matcher: matcher,
            default_target_id: target.target_id.clone(),
            policy_profile: DEFAULT_POLICY_PROFILE.to_string(),
            active: true,
            created_at: Some(Utc::now()),
        },
        true,
    )
}

fn find_matching_binding<'a>(
    snapshot: &'a RegistrySnapshot,
    selection: &ActivationSelection,
    target: &RegistryProjectionTarget,
) -> Option<&'a RegistryWorkspaceBinding> {
    let matcher = workspace_matcher(selection);
    snapshot.bindings.bindings.iter().find(|binding| {
        binding.agent == selection.agent
            && binding.profile_id == selection.profile
            && binding.default_target_id == target.target_id
            && binding.workspace_matcher.kind == matcher.kind
            && binding.workspace_matcher.value == matcher.value
            && binding.active
    })
}

pub(crate) fn find_rule<'a>(
    snapshot: &'a RegistrySnapshot,
    binding: &RegistryWorkspaceBinding,
    target: &RegistryProjectionTarget,
    skill: &str,
) -> Option<&'a RegistryBindingRule> {
    snapshot.rules.rules.iter().find(|rule| {
        rule.binding_id == binding.binding_id
            && rule.skill_id == skill
            && rule.target_id == target.target_id
    })
}

pub(crate) fn find_projection<'a>(
    snapshot: &'a RegistrySnapshot,
    binding: &RegistryWorkspaceBinding,
    target: &RegistryProjectionTarget,
    skill: &str,
) -> Option<&'a RegistryProjectionInstance> {
    snapshot.projections.projections.iter().find(|projection| {
        projection.skill_id == skill
            && projection.binding_id.as_deref() == Some(&binding.binding_id)
            && projection.target_id == target.target_id
    })
}

fn workspace_matcher(selection: &ActivationSelection) -> RegistryWorkspaceMatcher {
    match selection.scope {
        ActivationScope::User => RegistryWorkspaceMatcher {
            kind: crate::core::vocab::MatcherKind::Name,
            value: "user".to_string(),
        },
        ActivationScope::Project => RegistryWorkspaceMatcher {
            kind: crate::core::vocab::MatcherKind::PathPrefix,
            value: selection
                .workspace
                .as_ref()
                .expect("project scope has workspace")
                .display()
                .to_string(),
        },
    }
}

fn default_target_path(
    ctx: &crate::state::AppContext,
    selection: &ActivationSelection,
) -> std::result::Result<PathBuf, CommandFailure> {
    let scope = match selection.scope {
        ActivationScope::User => "user",
        ActivationScope::Project => "project",
    };
    let implicit_workspace = selection
        .workspace
        .is_none()
        .then(|| std::env::current_dir().map_err(map_io))
        .transpose()?;
    let workspace = selection
        .workspace
        .as_deref()
        .or(implicit_workspace.as_deref())
        .ok_or_else(|| CommandFailure::new(ErrorCode::IoError, "workspace is unavailable"))?;
    let adapters = load_agent_adapters(ctx)?;
    if let Some(adapter) = adapters.adapter_for_agent(&selection.agent)
        && adapter.has_discovery_root_for_scope(scope)
    {
        if let Some(root) =
            built_in_projection_root(ctx, adapter, scope, workspace, &selection.skill)?
        {
            return Ok(root);
        }
        match preferred_discovery_root(adapter, scope, workspace) {
            Ok(root) => return Ok(root.path),
            Err(_err) if adapter.source == SOURCE_BUILT_IN => {}
            Err(err) => return Err(err),
        }
    }
    match selection.scope {
        ActivationScope::User => resolve_agent_skill_dirs(&ctx.root)
            .all
            .into_iter()
            .find(|dir| dir.agent == selection.agent)
            .map(|dir| dir.path)
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("unknown agent '{}'", selection.agent),
                )
            }),
        ActivationScope::Project => {
            let workspace = selection.workspace.as_ref().ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--workspace is required when --scope project",
                )
            })?;
            if selection.agent == "codex" {
                Ok(workspace.join(".agents/skills"))
            } else {
                Ok(workspace
                    .join(format!(".{}", selection.agent))
                    .join("skills"))
            }
        }
    }
}

pub(super) fn workspace_for_scope(
    scope: ActivationScope,
    workspace: Option<PathBuf>,
) -> std::result::Result<Option<PathBuf>, CommandFailure> {
    let Some(raw) = workspace else {
        return if matches!(scope, ActivationScope::Project) {
            Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--workspace is required when --scope project",
            ))
        } else {
            Ok(None)
        };
    };
    let absolute = if raw.is_absolute() {
        raw
    } else {
        std::env::current_dir().map_err(map_io)?.join(raw)
    };
    Ok(Some(normalize_existing_or_raw(&absolute)))
}

pub(super) fn optional_snapshot(
    ctx: &crate::state::AppContext,
) -> std::result::Result<RegistrySnapshot, CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    Ok(paths
        .maybe_load_snapshot()
        .map_err(map_registry_state)?
        .unwrap_or_else(empty_snapshot))
}

fn empty_snapshot() -> RegistrySnapshot {
    RegistrySnapshot {
        schema: crate::state_model::RegistrySchemaFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            created_at: Utc::now(),
            writer: format!("loom/{}", env!("CARGO_PKG_VERSION")),
        },
        targets: empty_targets_file(),
        bindings: empty_bindings_file(),
        rules: empty_rules_file(),
        projections: empty_projections_file(),
        operations: Vec::new(),
        checkpoint: RegistryOpsCheckpoint {
            schema_version: REGISTRY_SCHEMA_VERSION,
            last_scanned_op_id: None,
            last_acked_op_id: None,
            updated_at: Utc::now(),
        },
    }
}

pub(super) fn normalize_agent(agent: &str) -> std::result::Result<String, CommandFailure> {
    let normalized = agent.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || !normalized
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--agent must match [a-z0-9-]+",
        ));
    }
    Ok(normalized)
}

pub(super) fn normalize_existing_or_raw(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn target_path_matches(stored: &str, expected: &Path) -> bool {
    normalize_existing_or_raw(Path::new(stored)) == normalize_existing_or_raw(expected)
}

fn unique_activation_binding_id(
    bindings: &crate::state_model::RegistryBindingsFile,
    selection: &ActivationSelection,
    matcher: &RegistryWorkspaceMatcher,
) -> String {
    let scope_token = match selection.scope {
        ActivationScope::User => "user".to_string(),
        ActivationScope::Project => Path::new(&matcher.value)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("project")
            .to_string(),
    };
    let base = format!(
        "bind_{}_{}_{}",
        slugify(&selection.agent),
        slugify(&selection.profile),
        slugify(&scope_token)
    );
    if !bindings
        .bindings
        .iter()
        .any(|binding| binding.binding_id == base)
    {
        return base;
    }
    for index in 2..1000 {
        let candidate = format!("{base}_{index}");
        if !bindings
            .bindings
            .iter()
            .any(|binding| binding.binding_id == candidate)
        {
            return candidate;
        }
    }
    format!("{}_{}", base, uuid::Uuid::new_v4().simple())
}

pub(super) fn ensure_skill_exists_without_layout(
    ctx: &crate::state::AppContext,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    if !ctx.skill_path(skill).is_dir() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    Ok(())
}

pub(super) fn scope_str(scope: ActivationScope) -> &'static str {
    match scope {
        ActivationScope::User => "user",
        ActivationScope::Project => "project",
    }
}
