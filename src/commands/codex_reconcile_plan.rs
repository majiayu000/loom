use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::state::AppContext;
use crate::state_model::{
    RegistryBindingRule, RegistryProjectionInstance, RegistryProjectionTarget, RegistrySnapshot,
};
use crate::types::ErrorCode;

use super::CommandFailure;
use super::codex_config::{CodexConfigLoad, CodexConfigView, load_codex_config};
use super::codex_visibility::{
    CODEX_AGENT, CodexReconcileAction, CodexReconcilePlan, CodexReconcileRequest, RUNTIME_ENTRIES,
    normalize_existing_or_raw, path_exists_or_symlink, projection_path_is_safe_symlink,
};

pub(crate) fn plan_codex_reconcile(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
    request: &CodexReconcileRequest,
) -> std::result::Result<Vec<CodexReconcilePlan>, CommandFailure> {
    let config = load_codex_config()?;
    let targets = select_codex_targets(snapshot, request)?;
    let mut plans = Vec::with_capacity(targets.len());
    for target in targets {
        plans.push(plan_target(ctx, snapshot, request, &target, &config));
    }
    Ok(plans)
}

fn plan_target(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
    request: &CodexReconcileRequest,
    target: &RegistryProjectionTarget,
    config: &CodexConfigLoad,
) -> CodexReconcilePlan {
    let target_path = PathBuf::from(&target.path);
    let desired = desired_rules_for_target(snapshot, target);
    let desired_skills = desired.keys().cloned().collect::<BTreeSet<_>>();
    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    if request.allowlist_path.is_some() {
        warnings.push("allowlist is accepted for future legacy cleanup but not applied by this reconcile slice".to_string());
    }
    if target.ownership != "managed" {
        actions.push(action(
            "manual_review",
            None,
            Some(target.path.clone()),
            false,
            false,
            "target is not managed by Loom",
            json!({"target_id": target.target_id, "ownership": target.ownership}),
        ));
    }

    for (skill, rule) in &desired {
        plan_desired_projection(ctx, snapshot, target, rule, &mut actions);
        if let CodexConfigLoad::Parsed(view) = config {
            add_config_disable_actions(ctx, view, skill, &mut actions);
        }
    }
    if let CodexConfigLoad::Malformed(error) = config {
        actions.push(action(
            "manual_review",
            None,
            None,
            false,
            true,
            "Codex config is malformed and blocks config repair",
            json!({"config_path": &error.path, "error": &error.error}),
        ));
    }

    let mut stale_record_ids = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(&target_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if RUNTIME_ENTRIES.contains(&name.as_str()) {
                actions.push(action(
                    "preserve_runtime_entry",
                    Some(name),
                    Some(path.display().to_string()),
                    true,
                    false,
                    "Codex runtime entry is preserved",
                    json!({"target_id": target.target_id}),
                ));
                continue;
            }
            if desired_skills.contains(&name) {
                continue;
            }
            let matching_record = projection_for_path(snapshot, target, &path);
            if stale_entry_is_safe(ctx, matching_record, &name, &path) {
                actions.push(action(
                    "remove_stale_projection",
                    Some(name.clone()),
                    Some(path.display().to_string()),
                    true,
                    false,
                    "stale Loom-owned symlink is not desired by any active binding on this target",
                    json!({"target_id": target.target_id}),
                ));
                if let Some(record) = matching_record {
                    stale_record_ids.insert(record.instance_id.clone());
                    actions.push(action(
                        "remove_stale_record",
                        Some(record.skill_id.clone()),
                        Some(record.materialized_path.clone()),
                        true,
                        false,
                        "stale projection record has no desired active rule",
                        json!({"instance_id": record.instance_id, "target_id": target.target_id}),
                    ));
                }
            } else {
                actions.push(action(
                    "preserve_external_entry",
                    Some(name),
                    Some(path.display().to_string()),
                    true,
                    false,
                    "entry is not a safe Loom-owned stale symlink",
                    json!({"target_id": target.target_id}),
                ));
            }
        }
    }

    for record in snapshot
        .projections
        .projections
        .iter()
        .filter(|record| record.target_id == target.target_id)
    {
        if desired_skills.contains(&record.skill_id)
            || stale_record_ids.contains(&record.instance_id)
        {
            continue;
        }
        let path = PathBuf::from(&record.materialized_path);
        if !path_exists_or_symlink(&path) {
            actions.push(action(
                "remove_stale_record",
                Some(record.skill_id.clone()),
                Some(record.materialized_path.clone()),
                true,
                false,
                "stale projection record points to a missing filesystem entry",
                json!({"instance_id": record.instance_id, "target_id": target.target_id}),
            ));
        }
    }

    let restart_required = actions.iter().any(|item| {
        matches!(
            item.category.as_str(),
            "create_projection"
                | "repair_projection"
                | "remove_stale_projection"
                | "fix_config_disable"
        )
    });
    if restart_required {
        warnings.push("restart_required_after_apply".to_string());
    }
    let safe_to_apply = actions
        .iter()
        .all(|item| item.safe && (!item.requires_fix_config || request.fix_config));

    CodexReconcilePlan {
        agent: CODEX_AGENT.to_string(),
        binding_id: request.binding_id.clone(),
        target_id: target.target_id.clone(),
        target_path: target.path.clone(),
        dry_run: request.dry_run,
        safe_to_apply,
        actions,
        warnings,
        restart_required,
    }
}

fn plan_desired_projection(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
    target: &RegistryProjectionTarget,
    rule: &RegistryBindingRule,
    actions: &mut Vec<CodexReconcileAction>,
) {
    if rule.method != "symlink" {
        actions.push(action(
            "manual_review",
            Some(rule.skill_id.clone()),
            None,
            false,
            false,
            "Codex reconcile only repairs symlink active-view projections",
            json!({"method": rule.method, "target_id": target.target_id}),
        ));
        return;
    }
    let source = ctx.skill_path(&rule.skill_id);
    if !source.join("SKILL.md").is_file() {
        actions.push(action(
            "manual_review",
            Some(rule.skill_id.clone()),
            Some(source.display().to_string()),
            false,
            false,
            "desired active skill source or SKILL.md is missing",
            json!({"target_id": target.target_id}),
        ));
        return;
    }
    let expected = PathBuf::from(&target.path).join(&rule.skill_id);
    let record = snapshot.projections.projections.iter().find(|projection| {
        projection.target_id == target.target_id && projection.skill_id == rule.skill_id
    });
    match fs::symlink_metadata(&expected) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => actions.push(action(
            "create_projection",
            Some(rule.skill_id.clone()),
            Some(expected.display().to_string()),
            true,
            false,
            "desired active skill is missing from Codex target",
            json!({"binding_id": rule.binding_id, "target_id": target.target_id}),
        )),
        Err(err) => actions.push(action(
            "manual_review",
            Some(rule.skill_id.clone()),
            Some(expected.display().to_string()),
            false,
            false,
            "projection path could not be inspected",
            json!({"error": err.to_string(), "target_id": target.target_id}),
        )),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if projection_path_is_safe_symlink(&expected, &source) {
                return;
            }
            if record.is_some_and(|record| record.method == "symlink") {
                actions.push(action(
                    "repair_projection",
                    Some(rule.skill_id.clone()),
                    Some(expected.display().to_string()),
                    true,
                    false,
                    "recorded Loom-owned projection symlink no longer points to source",
                    json!({"binding_id": rule.binding_id, "target_id": target.target_id}),
                ));
            } else {
                actions.push(action(
                    "manual_review",
                    Some(rule.skill_id.clone()),
                    Some(expected.display().to_string()),
                    false,
                    false,
                    "projection symlink is not proven Loom-owned",
                    json!({"target_id": target.target_id}),
                ));
            }
        }
        Ok(_) => actions.push(action(
            "manual_review",
            Some(rule.skill_id.clone()),
            Some(expected.display().to_string()),
            false,
            false,
            "projection path exists but is not a symlink",
            json!({"target_id": target.target_id}),
        )),
    }
}

fn add_config_disable_actions(
    ctx: &AppContext,
    view: &CodexConfigView,
    skill: &str,
    actions: &mut Vec<CodexReconcileAction>,
) {
    let skill_file = ctx.skill_path(skill).join("SKILL.md");
    for entry in &view.entries {
        if !entry.is_disabled() {
            continue;
        }
        let Some(match_kind) = entry.matches_skill(skill, &skill_file) else {
            continue;
        };
        actions.push(action(
            "fix_config_disable",
            Some(skill.to_string()),
            Some(view.path.display().to_string()),
            true,
            true,
            "active skill is disabled by Codex config",
            json!({
                "entry_index": entry.index,
                "match_kind": match_kind,
                "config_path": &view.path,
                "skill_file": skill_file
            }),
        ));
    }
}

fn select_codex_targets(
    snapshot: &RegistrySnapshot,
    request: &CodexReconcileRequest,
) -> std::result::Result<Vec<RegistryProjectionTarget>, CommandFailure> {
    if let Some(binding_id) = request.binding_id.as_deref() {
        let binding = snapshot.binding(binding_id).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{}' not found", binding_id),
            )
        })?;
        if binding.agent != CODEX_AGENT {
            return Err(CommandFailure::new(
                ErrorCode::TargetAgentMismatch,
                format!(
                    "binding '{}' is for agent '{}' not codex",
                    binding_id, binding.agent
                ),
            ));
        }
        if let Some(target_id) = request.target_id.as_deref()
            && target_id != binding.default_target_id
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "binding '{}' points to target '{}' not '{}'",
                    binding_id, binding.default_target_id, target_id
                ),
            ));
        }
        return Ok(vec![codex_target(snapshot, &binding.default_target_id)?]);
    }
    if let Some(target_id) = request.target_id.as_deref() {
        return Ok(vec![codex_target(snapshot, target_id)?]);
    }
    let targets = snapshot
        .targets
        .targets
        .iter()
        .filter(|target| target.agent == CODEX_AGENT)
        .cloned()
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::TargetNotFound,
            "no Codex target registered",
        ));
    }
    Ok(targets)
}

fn codex_target(
    snapshot: &RegistrySnapshot,
    target_id: &str,
) -> std::result::Result<RegistryProjectionTarget, CommandFailure> {
    let target = snapshot.target(target_id).cloned().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::TargetNotFound,
            format!("target '{}' not found", target_id),
        )
    })?;
    if target.agent != CODEX_AGENT {
        return Err(CommandFailure::new(
            ErrorCode::TargetAgentMismatch,
            format!(
                "target '{}' is for agent '{}' not codex",
                target_id, target.agent
            ),
        ));
    }
    Ok(target)
}

fn desired_rules_for_target<'a>(
    snapshot: &'a RegistrySnapshot,
    target: &RegistryProjectionTarget,
) -> BTreeMap<String, &'a RegistryBindingRule> {
    let active_bindings = snapshot
        .bindings
        .bindings
        .iter()
        .filter(|binding| {
            binding.agent == CODEX_AGENT
                && binding.active
                && binding.default_target_id == target.target_id
        })
        .map(|binding| binding.binding_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut desired = BTreeMap::new();
    for rule in &snapshot.rules.rules {
        if rule.target_id == target.target_id && active_bindings.contains(rule.binding_id.as_str())
        {
            desired.entry(rule.skill_id.clone()).or_insert(rule);
        }
    }
    desired
}

fn projection_for_path<'a>(
    snapshot: &'a RegistrySnapshot,
    target: &RegistryProjectionTarget,
    path: &Path,
) -> Option<&'a RegistryProjectionInstance> {
    let normalized = normalize_existing_or_raw(path);
    snapshot.projections.projections.iter().find(|projection| {
        projection.target_id == target.target_id
            && normalize_existing_or_raw(Path::new(&projection.materialized_path)) == normalized
    })
}

fn stale_entry_is_safe(
    ctx: &AppContext,
    record: Option<&RegistryProjectionInstance>,
    entry_name: &str,
    path: &Path,
) -> bool {
    if let Some(record) = record
        && record.method == "symlink"
        && fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
    {
        return true;
    }
    projection_path_is_safe_symlink(path, &ctx.skill_path(entry_name))
}

fn action(
    category: &str,
    skill: Option<String>,
    path: Option<String>,
    safe: bool,
    requires_fix_config: bool,
    reason: &str,
    details: Value,
) -> CodexReconcileAction {
    CodexReconcileAction {
        category: category.to_string(),
        skill,
        path,
        safe,
        requires_fix_config,
        reason: reason.to_string(),
        details,
    }
}
