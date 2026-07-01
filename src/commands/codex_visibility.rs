use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};

use crate::state::AppContext;
use crate::state_model::{
    RegistryBindingRule, RegistryProjectionTarget, RegistrySnapshot, RegistryStatePaths,
    RegistryWorkspaceBinding,
};
use crate::types::ErrorCode;

use super::CommandFailure;
use super::codex_config::{CodexConfigLoad, load_codex_config};
use super::helpers::{map_arg, map_registry_state, validate_skill_name};

pub(crate) const CODEX_AGENT: &str = "codex";
pub(crate) const RUNTIME_ENTRIES: &[&str] = &[".system", "codex-primary-runtime"];

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexVisibilityReport {
    pub(crate) skill: String,
    pub(crate) agent: String,
    pub(crate) visible: bool,
    pub(crate) checks: Vec<CodexVisibilityCheck>,
    pub(crate) next_actions: Vec<String>,
    pub(crate) restart_required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexVisibilityCheck {
    pub(crate) id: String,
    pub(crate) ok: bool,
    pub(crate) severity: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub(crate) details: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) next_action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexReconcilePlan {
    pub(crate) agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) binding_id: Option<String>,
    pub(crate) target_id: String,
    pub(crate) target_path: String,
    pub(crate) dry_run: bool,
    pub(crate) safe_to_apply: bool,
    pub(crate) actions: Vec<CodexReconcileAction>,
    pub(crate) warnings: Vec<String>,
    pub(crate) restart_required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexReconcileAction {
    pub(crate) category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
    pub(crate) safe: bool,
    pub(crate) requires_fix_config: bool,
    pub(crate) reason: String,
    pub(crate) details: Value,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexReconcileRequest {
    pub(crate) binding_id: Option<String>,
    pub(crate) target_id: Option<String>,
    pub(crate) allowlist_path: Option<PathBuf>,
    pub(crate) dry_run: bool,
    pub(crate) fix_config: bool,
}

pub(crate) fn build_codex_visibility_report(
    ctx: &AppContext,
    skill: &str,
    workspace: Option<&Path>,
    profile: Option<&str>,
) -> std::result::Result<CodexVisibilityReport, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let paths = RegistryStatePaths::from_app_context(ctx);
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    if !ctx.skill_path(skill).is_dir()
        && !snapshot
            .as_ref()
            .is_some_and(|snapshot| skill_is_referenced(snapshot, skill))
    {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    let config = load_codex_config()?;
    Ok(build_visibility_report_from_parts(
        ctx,
        skill,
        snapshot.as_ref(),
        config,
        workspace,
        profile,
    ))
}

pub(crate) fn normalize_existing_or_raw(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn projection_path_is_safe_symlink(path: &Path, skill_src: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(link_target) = fs::read_link(path) else {
        return false;
    };
    let actual = if link_target.is_absolute() {
        link_target
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(link_target)
    };
    normalize_existing_or_raw(&actual) == normalize_existing_or_raw(skill_src)
}

pub(crate) fn path_exists_or_symlink(path: &Path) -> bool {
    path.exists() || fs::symlink_metadata(path).is_ok()
}

fn build_visibility_report_from_parts(
    ctx: &AppContext,
    skill: &str,
    snapshot: Option<&RegistrySnapshot>,
    config: CodexConfigLoad,
    workspace: Option<&Path>,
    profile: Option<&str>,
) -> CodexVisibilityReport {
    let source_path = ctx.skill_path(skill);
    let skill_file = source_path.join("SKILL.md");
    let source_exists = source_path.is_dir();
    let skill_file_exists = skill_file.is_file();
    let mut checks = vec![
        check(
            "codex_source_exists",
            source_exists,
            "error",
            if source_exists {
                "source skill exists"
            } else {
                "source skill is missing"
            },
            json!({"source_path": source_path}),
            Some(format!(
                "restore skills/{skill} or remove stale active rules"
            )),
        ),
        check(
            "codex_skill_file_exists",
            skill_file_exists,
            "error",
            if skill_file_exists {
                "source SKILL.md exists"
            } else {
                "source SKILL.md is missing"
            },
            json!({"skill_file": skill_file}),
            Some(format!("add skills/{skill}/SKILL.md before projecting")),
        ),
    ];

    let mut rule_count = 0;
    let mut projection_ok = false;
    if let Some(snapshot) = snapshot {
        let rules = active_codex_rules_for_skill(snapshot, skill, workspace, profile);
        rule_count = rules.len();
        checks.push(check(
            "codex_active_rule_exists",
            !rules.is_empty(),
            "error",
            if rules.is_empty() {
                "no active Codex rule selects this skill"
            } else {
                "active Codex rule selects this skill"
            },
            json!({"rule_count": rules.len()}),
            Some(format!("loom skill activate {skill} --agent codex")),
        ));
        for rule in rules {
            add_rule_visibility_checks(ctx, snapshot, &rule, &mut checks, &mut projection_ok);
        }
    } else {
        checks.push(check(
            "codex_registry_snapshot_exists",
            false,
            "error",
            "registry snapshot is missing",
            json!({}),
            Some("run loom init before Codex visibility checks".to_string()),
        ));
    }

    let disabled = add_config_checks(skill, &skill_file, config, &mut checks);
    checks.push(check(
        "codex_restart_required",
        true,
        "warning",
        "current Codex sessions are not claimed to hot-reload visibility changes",
        json!({"restart_required_after_apply": true}),
        None,
    ));

    let visible = source_exists
        && skill_file_exists
        && rule_count > 0
        && projection_ok
        && !disabled
        && !checks
            .iter()
            .any(|check| check.severity == "error" && !check.ok);
    let mut next_actions = BTreeSet::new();
    for check in &checks {
        if !check.ok
            && let Some(next) = &check.next_action
        {
            next_actions.insert(next.clone());
        }
    }
    if disabled {
        next_actions.insert("loom codex reconcile --apply --fix-config".to_string());
        next_actions.insert("restart Codex or open a new session".to_string());
    }

    CodexVisibilityReport {
        skill: skill.to_string(),
        agent: CODEX_AGENT.to_string(),
        visible,
        checks,
        next_actions: next_actions.into_iter().collect(),
        restart_required: false,
    }
}

fn add_rule_visibility_checks(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
    rule: &RegistryBindingRule,
    checks: &mut Vec<CodexVisibilityCheck>,
    projection_ok: &mut bool,
) {
    let Some(target) = snapshot.target(&rule.target_id) else {
        checks.push(check(
            &format!("codex_target_exists:{}", rule.target_id),
            false,
            "error",
            "Codex target referenced by active rule is missing",
            json!({"target_id": rule.target_id}),
            Some("recreate the target or remove the stale rule".to_string()),
        ));
        return;
    };
    let target_path = PathBuf::from(&target.path);
    checks.push(check(
        &format!("codex_target_path_exists:{}", target.target_id),
        target_path.is_dir(),
        "error",
        if target_path.is_dir() {
            "Codex target path exists"
        } else {
            "Codex target path is missing"
        },
        json!({"target_id": target.target_id, "target_path": target.path}),
        Some("recreate the target path or run loom codex reconcile --apply".to_string()),
    ));
    let projection_path = target_path.join(&rule.skill_id);
    checks.push(check(
        &format!("codex_projection_path_exists:{}", target.target_id),
        path_exists_or_symlink(&projection_path),
        "error",
        if path_exists_or_symlink(&projection_path) {
            "projection path exists"
        } else {
            "projection path is missing"
        },
        json!({"projection_path": projection_path}),
        Some("loom codex reconcile --apply".to_string()),
    ));
    let is_symlink = fs::symlink_metadata(&projection_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    checks.push(check(
        &format!("codex_projection_is_symlink:{}", target.target_id),
        is_symlink,
        "error",
        if is_symlink {
            "projection is a symlink"
        } else {
            "projection is not a symlink"
        },
        json!({"projection_path": projection_path}),
        Some("inspect the projection path before repair".to_string()),
    ));
    let points_to_source =
        projection_path_is_safe_symlink(&projection_path, &ctx.skill_path(&rule.skill_id));
    if points_to_source {
        *projection_ok = true;
    }
    checks.push(check(
        &format!("codex_projection_points_to_source:{}", target.target_id),
        points_to_source,
        "error",
        if points_to_source {
            "projection symlink resolves to source skill"
        } else {
            "projection symlink does not resolve to source skill"
        },
        json!({
            "projection_path": projection_path,
            "source_path": ctx.skill_path(&rule.skill_id)
        }),
        Some("loom codex reconcile --apply".to_string()),
    ));
    add_entry_classification_checks(ctx, target, &rule.skill_id, checks);
}

fn add_entry_classification_checks(
    ctx: &AppContext,
    target: &RegistryProjectionTarget,
    skill: &str,
    checks: &mut Vec<CodexVisibilityCheck>,
) {
    let target_path = Path::new(&target.path);
    let Ok(entries) = fs::read_dir(target_path) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if RUNTIME_ENTRIES.contains(&name.as_str()) {
            checks.push(check(
                &format!("codex_runtime_entry_classification:{name}"),
                true,
                "warning",
                "Codex runtime entry is preserved",
                json!({"target_path": target.path, "entry": name}),
                None,
            ));
            continue;
        }
        if name == skill {
            continue;
        }
        let path = entry.path();
        let loom_owned = projection_path_is_safe_symlink(&path, &ctx.skill_path(&name));
        checks.push(check(
            &format!("codex_external_entry_classification:{name}"),
            true,
            "warning",
            if loom_owned {
                "inactive Loom-owned entry is reported for reconcile"
            } else {
                "external Codex entry is preserved"
            },
            json!({"target_path": target.path, "entry": name, "loom_owned_symlink": loom_owned}),
            None,
        ));
    }
}

fn add_config_checks(
    skill: &str,
    skill_file: &Path,
    config: CodexConfigLoad,
    checks: &mut Vec<CodexVisibilityCheck>,
) -> bool {
    match config {
        CodexConfigLoad::Malformed(error) => {
            checks.push(check(
                "codex_config_parse",
                false,
                "error",
                "Codex config is malformed",
                json!({"config_path": error.path, "error": error.error}),
                Some("repair Codex config TOML before running --fix-config".to_string()),
            ));
            true
        }
        CodexConfigLoad::Parsed(view) => {
            let mut path_disabled = Vec::new();
            let mut name_disabled = Vec::new();
            for entry in &view.entries {
                if !entry.is_disabled() {
                    continue;
                }
                match entry.matches_skill(skill, skill_file) {
                    Some("path") => path_disabled.push(entry.index),
                    Some("name") => name_disabled.push(entry.index),
                    _ => {}
                }
            }
            checks.push(check(
                "codex_config_not_disabled_by_path",
                path_disabled.is_empty(),
                "error",
                if path_disabled.is_empty() {
                    "canonical SKILL.md is not disabled by path"
                } else {
                    "canonical SKILL.md is disabled in Codex config"
                },
                json!({"config_path": view.path, "entry_indices": path_disabled}),
                Some("loom codex reconcile --apply --fix-config, then restart Codex".to_string()),
            ));
            checks.push(check(
                "codex_config_not_disabled_by_name",
                name_disabled.is_empty(),
                "error",
                if name_disabled.is_empty() {
                    "skill name is not disabled by Codex config"
                } else {
                    "skill name is disabled in Codex config"
                },
                json!({"config_path": view.path, "entry_indices": name_disabled}),
                Some("loom codex reconcile --apply --fix-config, then restart Codex".to_string()),
            ));
            !path_disabled.is_empty() || !name_disabled.is_empty()
        }
    }
}

fn active_codex_rules_for_skill(
    snapshot: &RegistrySnapshot,
    skill: &str,
    workspace: Option<&Path>,
    profile: Option<&str>,
) -> Vec<RegistryBindingRule> {
    snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.skill_id == skill)
        .filter_map(|rule| {
            let binding = snapshot.binding(&rule.binding_id)?;
            let target = snapshot.target(&rule.target_id)?;
            if binding.agent == CODEX_AGENT
                && binding.active
                && target.agent == CODEX_AGENT
                && profile.is_none_or(|profile| binding.profile_id == profile)
                && workspace.is_none_or(|workspace| binding_matches_workspace(binding, workspace))
            {
                Some(rule.clone())
            } else {
                None
            }
        })
        .collect()
}

fn binding_matches_workspace(binding: &RegistryWorkspaceBinding, workspace: &Path) -> bool {
    let expected = normalize_existing_or_raw(workspace);
    let matcher = &binding.workspace_matcher;
    match matcher.kind.as_str() {
        "path_prefix" => expected.starts_with(normalize_existing_or_raw(Path::new(&matcher.value))),
        "exact_path" => expected == normalize_existing_or_raw(Path::new(&matcher.value)),
        "name" => true,
        _ => false,
    }
}

fn skill_is_referenced(snapshot: &RegistrySnapshot, skill: &str) -> bool {
    snapshot
        .rules
        .rules
        .iter()
        .any(|rule| rule.skill_id == skill)
        || snapshot
            .projections
            .projections
            .iter()
            .any(|projection| projection.skill_id == skill)
}

fn check(
    id: &str,
    ok: bool,
    failure_severity: &str,
    message: &str,
    details: Value,
    next_action: Option<String>,
) -> CodexVisibilityCheck {
    CodexVisibilityCheck {
        id: id.to_string(),
        ok,
        severity: if ok { "info" } else { failure_severity }.to_string(),
        message: message.to_string(),
        details,
        next_action: if ok { None } else { next_action },
    }
}
