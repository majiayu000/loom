use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::{AgentKind, SkillDiagnoseArgs, SkillOnlyArgs};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::state_model::{RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths};
use crate::types::{ErrorCode, PendingOp};

use super::codex_visibility::build_codex_visibility_report;
use super::helpers::{map_arg, map_registry_state, validate_skill_name};
use super::history_cmds::operation_mentions_skill as registry_operation_mentions_skill;
use super::skill_deps::skill_dependency_report;
use super::skill_verify::{
    drifted_paths_under, head_tree_oid_for_path, last_commit_for_path, last_saved_commit_for_skill,
};
use super::{App, CommandFailure, SkillLintMode, SkillLintReport, lint_skill_source};

const MAX_DRIFTED_PATHS: usize = 100;
const MAX_RELATED_OPS: usize = 10;

impl App {
    pub fn cmd_skill_diagnose<T: SkillDiagnoseRequest>(
        &self,
        args: &T,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let skill = args.skill();
        validate_skill_name(skill).map_err(map_arg)?;
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
        let (mut data, meta) = build_skill_diagnosis(&self.ctx, skill, snapshot.as_ref())?;
        if let Some(agent) = args.agent() {
            if agent != AgentKind::Codex {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "skill diagnose --agent currently supports only codex",
                ));
            }
            attach_codex_visibility(&self.ctx, skill, &mut data)?;
        }
        Ok((data, meta))
    }
}

pub trait SkillDiagnoseRequest {
    fn skill(&self) -> &str;
    fn agent(&self) -> Option<AgentKind>;
}

impl SkillDiagnoseRequest for SkillDiagnoseArgs {
    fn skill(&self) -> &str {
        &self.skill
    }

    fn agent(&self) -> Option<AgentKind> {
        self.agent
    }
}

impl SkillDiagnoseRequest for SkillOnlyArgs {
    fn skill(&self) -> &str {
        &self.skill
    }

    fn agent(&self) -> Option<AgentKind> {
        None
    }
}

fn attach_codex_visibility(
    ctx: &AppContext,
    skill: &str,
    data: &mut Value,
) -> std::result::Result<(), CommandFailure> {
    let report = build_codex_visibility_report(ctx, skill, None, None)?;
    let report_value = json!(report);
    if let Some(checks) = data.get_mut("checks").and_then(Value::as_array_mut)
        && let Some(codex_checks) = report_value.get("checks").and_then(Value::as_array)
    {
        for check in codex_checks {
            let mut check = check.clone();
            if let Some(object) = check.as_object_mut() {
                object.insert("section".to_string(), Value::String("codex".to_string()));
            }
            checks.push(check);
        }
    }
    data["related"]["codex_visibility"] = report_value;
    let error_count = data["checks"]
        .as_array()
        .map(|checks| checks_with_severity(checks, "error"))
        .unwrap_or(0);
    let warning_count = data["checks"]
        .as_array()
        .map(|checks| checks_with_severity(checks, "warning"))
        .unwrap_or(0);
    let status = if error_count > 0 {
        "blocked"
    } else if warning_count > 0 {
        "attention"
    } else {
        "healthy"
    };
    data["healthy"] = Value::Bool(status == "healthy");
    data["status"] = Value::String(status.to_string());
    data["summary"]["failed_check_count"] = json!(error_count);
    data["summary"]["warning_check_count"] = json!(warning_count);
    Ok(())
}

fn build_skill_diagnosis(
    ctx: &AppContext,
    skill: &str,
    snapshot: Option<&RegistrySnapshot>,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    let source_path = ctx.skill_path(skill);
    let source_exists = source_path.is_dir();
    let referenced = source_exists || snapshot.is_some_and(|s| skill_is_referenced(s, skill));
    if !referenced {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }

    let mut checks = Vec::new();
    let mut bindings = Vec::new();
    let mut rules = Vec::new();
    let mut targets = Vec::new();
    let mut projections = Vec::new();
    let mut recent_ops = Vec::new();
    let mut pending_ops = Vec::new();
    let lint_report = lint_skill_source(&source_path, skill, SkillLintMode::Compat);
    let dependencies = source_exists
        .then(|| skill_dependency_report(ctx, skill, None, None))
        .transpose()?;

    add_source_checks(
        ctx,
        skill,
        &source_path,
        source_exists,
        &lint_report,
        &mut checks,
    );
    add_git_checks(ctx, skill, source_exists, &mut checks);

    if let Some(snapshot) = snapshot {
        let mut rule_target_ids = BTreeSet::new();
        let mut projection_only_target_ids = BTreeSet::new();

        for rule in snapshot
            .rules
            .rules
            .iter()
            .filter(|rule| rule.skill_id == skill)
        {
            rules.push(json!(rule));
            if let Some(binding) = snapshot.binding(&rule.binding_id) {
                bindings.push(json!(binding));
                add_binding_checks(snapshot, binding, &mut checks);
            }
            checks.push(check(
                "registry",
                &format!("binding_target_exists:{}", rule.binding_id),
                snapshot.binding(&rule.binding_id).is_some(),
                "error",
                "binding exists for skill rule",
                "remove or recreate the missing binding",
                json!({"binding_id": rule.binding_id}),
            ));
            add_target_checks(snapshot, &rule.target_id, &rule.method, &mut checks);
            rule_target_ids.insert(rule.target_id.clone());
        }

        for projection in snapshot
            .projections
            .projections
            .iter()
            .filter(|projection| projection.skill_id == skill)
        {
            projections.push(json!(projection));
            add_projection_checks(ctx, snapshot, projection, &mut checks);
            if !rule_target_ids.contains(&projection.target_id)
                && projection_only_target_ids.insert(projection.target_id.clone())
            {
                add_target_checks(
                    snapshot,
                    &projection.target_id,
                    &projection.method,
                    &mut checks,
                );
            }
        }

        for target in &snapshot.targets.targets {
            let used_by_rule = rules
                .iter()
                .any(|rule| rule["target_id"].as_str() == Some(&target.target_id));
            let used_by_projection = projections
                .iter()
                .any(|p| p["target_id"].as_str() == Some(&target.target_id));
            if used_by_rule || used_by_projection {
                targets.push(json!(target));
            }
        }

        recent_ops = snapshot
            .operations
            .iter()
            .rev()
            .filter(|op| registry_operation_mentions_skill(op, skill))
            .take(MAX_RELATED_OPS)
            .map(|op| {
                json!({
                    "op_id": op.op_id,
                    "intent": op.intent,
                    "status": op.status,
                    "last_error": op.last_error,
                    "created_at": op.created_at,
                    "updated_at": op.updated_at
                })
            })
            .collect();
        let failed_ops = recent_ops
            .iter()
            .filter(|op| !op["last_error"].is_null())
            .cloned()
            .collect::<Vec<_>>();
        checks.push(check(
            "operations",
            "recent_failed_ops",
            failed_ops.is_empty(),
            "warning",
            if failed_ops.is_empty() {
                "no recent failed operations for this skill"
            } else {
                "recent operations failed for this skill"
            },
            "inspect the failed operation details before retrying",
            json!({"operations": failed_ops}),
        ));
    }

    match ctx.read_pending_report() {
        Ok(report) => {
            pending_ops = report
                .ops
                .iter()
                .rev()
                .filter(|op| pending_op_mentions_skill(op, skill))
                .take(MAX_RELATED_OPS)
                .map(|op| {
                    json!({
                        "op_id": op.op_id,
                        "request_id": op.request_id,
                        "command": op.command,
                        "created_at": op.created_at,
                        "details": op.details
                    })
                })
                .collect();
            checks.push(check(
                "operations",
                "recent_pending_ops",
                pending_ops.is_empty(),
                "warning",
                if pending_ops.is_empty() {
                    "no pending operations for this skill"
                } else {
                    "pending operations exist for this skill"
                },
                "run loom ops list and resolve or retry pending work",
                json!({"operations": pending_ops}),
            ));
            checks.push(check(
                "operations",
                "pending_queue_warnings",
                report.warnings.is_empty(),
                "warning",
                if report.warnings.is_empty() {
                    "pending operation queue parsed cleanly"
                } else {
                    "pending operation queue has parse warnings"
                },
                "inspect state/pending_ops.jsonl and pending_ops_history for malformed entries",
                json!({"warnings": report.warnings}),
            ));
        }
        Err(err) => checks.push(check(
            "operations",
            "pending_queue_read",
            false,
            "error",
            "pending operation queue could not be read",
            "inspect state/pending_ops.jsonl and pending_ops_history permissions or file shape",
            json!({"error": err.to_string()}),
        )),
    }

    let error_count = checks_with_severity(&checks, "error");
    let warning_count = checks_with_severity(&checks, "warning");
    let status = if error_count > 0 {
        "blocked"
    } else if warning_count > 0 {
        "attention"
    } else {
        "healthy"
    };

    Ok((
        json!({
            "skill": skill,
            "healthy": status == "healthy",
            "status": status,
            "summary": {
                "source_status": if source_exists { "present" } else { "missing" },
                "binding_count": bindings.len(),
                "target_count": targets.len(),
                "projection_count": projections.len(),
                "failed_check_count": error_count,
                "warning_check_count": warning_count,
                "drifted_path_count": drifted_path_count(&checks),
                "recent_failed_op_count": recent_ops.iter().filter(|op| !op["last_error"].is_null()).count()
            },
            "checks": checks,
            "related": {
                "source": {
                    "path": source_path.display().to_string(),
                    "entrypoint": lint_report.entrypoint_path(),
                    "description": lint_report.description()
                },
                "bindings": bindings,
                "rules": rules,
                "targets": targets,
                "projections": projections,
                "recent_operations": recent_ops,
                "pending_operations": pending_ops,
                "dependencies": dependencies
            }
        }),
        Meta::default(),
    ))
}

fn add_source_checks(
    ctx: &AppContext,
    skill: &str,
    source_path: &Path,
    source_exists: bool,
    lint_report: &SkillLintReport,
    checks: &mut Vec<Value>,
) {
    checks.push(check(
        "source",
        "source_directory_exists",
        source_exists,
        "error",
        if source_exists {
            "source skill directory exists"
        } else {
            "source skill directory is missing"
        },
        "restore the source skill, re-add it, or clean orphaned references",
        json!({"path": source_path.display().to_string()}),
    ));
    let entrypoint = lint_report.entrypoint.file_name.as_deref();
    checks.push(check(
        "source",
        "skill_file_exists",
        entrypoint.is_some(),
        "error",
        if entrypoint.is_some() {
            "skill entrypoint exists"
        } else {
            "skill entrypoint is missing"
        },
        &format!("add skills/{skill}/SKILL.md or remove the non-compliant source"),
        json!({
            "accepted": ["SKILL.md", "skill.md"],
            "found": entrypoint
        }),
    ));
    let description = lint_report.description();
    checks.push(check(
        "source",
        "skill_frontmatter_description",
        description.is_some() || !source_exists,
        "warning",
        if description.is_some() || !source_exists {
            "skill description is available or source is absent"
        } else {
            "skill description is missing"
        },
        &format!("add description frontmatter to skills/{skill}/SKILL.md"),
        json!({"root": ctx.root.display().to_string()}),
    ));
    for finding in &lint_report.findings {
        checks.push(check(
            "source",
            &format!("skill_lint:{}", finding.id),
            false,
            &finding.severity,
            &finding.message,
            &finding.suggested_action,
            finding.details.clone(),
        ));
    }
}

fn add_git_checks(ctx: &AppContext, skill: &str, source_exists: bool, checks: &mut Vec<Value>) {
    if !source_exists {
        return;
    }
    let skill_rel = format!("skills/{skill}");

    match head_tree_oid_for_path(ctx, &skill_rel) {
        Ok(head_tree) => checks.push(check(
            "git",
            "source_tracked_at_head",
            head_tree.is_some(),
            "error",
            if head_tree.is_some() {
                "source skill is tracked at HEAD"
            } else {
                "source skill is not tracked at HEAD"
            },
            &format!("run loom skill save {skill}"),
            json!({"head_tree_oid": head_tree}),
        )),
        Err(err) => checks.push(check(
            "git",
            "source_tracked_at_head",
            false,
            "error",
            "source tracking could not be verified",
            "inspect the Git repository before saving or projecting this skill",
            json!({"error": err.to_string()}),
        )),
    }

    let last_commit = match last_saved_commit_for_skill(ctx, skill) {
        Ok(Some(commit)) => Some(commit),
        Ok(None) => match last_commit_for_path(ctx, &skill_rel) {
            Ok(commit) => commit,
            Err(err) => {
                push_source_drift_error(checks, None, err);
                return;
            }
        },
        Err(err) => {
            push_source_drift_error(checks, None, err);
            return;
        }
    };
    let mut drifted = match drifted_paths_under(ctx, &skill_rel, last_commit.as_deref()) {
        Ok(paths) => paths,
        Err(err) => {
            push_source_drift_error(checks, last_commit, err);
            return;
        }
    };
    let drifted_total = drifted.len();
    let truncated = drifted_total > MAX_DRIFTED_PATHS;
    drifted.truncate(MAX_DRIFTED_PATHS);
    checks.push(check(
        "git",
        "source_drift",
        drifted.is_empty(),
        "warning",
        if drifted.is_empty() {
            "source has no unsaved drift"
        } else {
            "source has unsaved drift"
        },
        &format!("run loom skill save {skill} or inspect loom skill diff"),
        json!({
            "last_source_commit": last_commit,
            "drifted_path_count": drifted_total,
            "drifted_paths": drifted,
            "drifted_paths_truncated": truncated
        }),
    ));
}

fn push_source_drift_error(
    checks: &mut Vec<Value>,
    last_commit: Option<String>,
    err: anyhow::Error,
) {
    checks.push(check(
        "git",
        "source_drift",
        false,
        "error",
        "source drift could not be verified",
        "inspect the Git repository before saving or projecting this skill",
        json!({
            "last_source_commit": last_commit,
            "error": err.to_string()
        }),
    ));
}

fn add_binding_checks(
    snapshot: &RegistrySnapshot,
    binding: &crate::state_model::RegistryWorkspaceBinding,
    checks: &mut Vec<Value>,
) {
    checks.push(check(
        "registry",
        &format!("binding_active:{}", binding.binding_id),
        binding.active,
        "warning",
        if binding.active {
            "binding is active"
        } else {
            "binding is inactive"
        },
        "reactivate or replace the binding before projecting",
        json!({"binding_id": binding.binding_id}),
    ));
    if let Some(default_target) = snapshot.target(&binding.default_target_id) {
        checks.push(check(
            "registry",
            &format!("binding_target_agent_match:{}", binding.binding_id),
            default_target.agent == binding.agent,
            "error",
            if default_target.agent == binding.agent {
                "binding and target agents match"
            } else {
                "binding and target agents do not match"
            },
            "point the binding at a target registered for the same agent",
            json!({
                "binding_id": binding.binding_id,
                "binding_agent": binding.agent,
                "target_id": default_target.target_id,
                "target_agent": default_target.agent
            }),
        ));
    }
}

fn add_target_checks(
    snapshot: &RegistrySnapshot,
    target_id: &str,
    method: &str,
    checks: &mut Vec<Value>,
) {
    let Some(target) = snapshot.target(target_id) else {
        checks.push(check(
            "targets",
            &format!("target_path_exists:{target_id}"),
            false,
            "error",
            "target is missing",
            "recreate the target or remove the rule",
            json!({"target_id": target_id}),
        ));
        return;
    };
    checks.push(check(
        "targets",
        &format!("target_path_exists:{}", target.target_id),
        Path::new(&target.path).exists(),
        "error",
        if Path::new(&target.path).exists() {
            "target path exists"
        } else {
            "target path is missing"
        },
        "recreate the target path or update the target",
        json!({"target_id": target.target_id, "path": target.path}),
    ));
    checks.push(check(
        "targets",
        &format!("target_ownership_writeable:{}", target.target_id),
        target.ownership == "managed",
        "warning",
        if target.ownership == "managed" {
            "target is managed"
        } else {
            "target is not managed"
        },
        "set target ownership to managed before writing projections",
        json!({"target_id": target.target_id, "ownership": target.ownership}),
    ));
    let capability_ok = match method {
        "symlink" => target.capabilities.symlink,
        "copy" | "materialize" => target.capabilities.copy,
        _ => false,
    };
    checks.push(check(
        "targets",
        &format!("target_capability:{}:{}", target.target_id, method),
        capability_ok,
        "error",
        "target supports projection method",
        "choose a supported projection method or update the target",
        json!({"target_id": target.target_id, "method": method}),
    ));
}

fn add_projection_checks(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
    projection: &RegistryProjectionInstance,
    checks: &mut Vec<Value>,
) {
    let materialized = Path::new(&projection.materialized_path);
    checks.push(check(
        "projection",
        &format!("projection_path_exists:{}", projection.instance_id),
        materialized.exists(),
        "error",
        if materialized.exists() {
            "projection path exists"
        } else {
            "projection path is missing"
        },
        "rerun loom skill project or clean the orphaned projection",
        json!({"instance_id": projection.instance_id, "path": projection.materialized_path}),
    ));
    checks.push(check(
        "projection",
        &format!("projection_source_exists:{}", projection.instance_id),
        ctx.skill_path(&projection.skill_id).exists(),
        "error",
        "projection source skill exists",
        "restore the source skill or clean the projection",
        json!({"instance_id": projection.instance_id, "skill_id": projection.skill_id}),
    ));
    checks.push(check(
        "projection",
        &format!("projection_health:{}", projection.instance_id),
        projection.health == "healthy",
        if projection.health == "drifted" || projection.health == "orphaned" {
            "warning"
        } else {
            "error"
        },
        if projection.health == "healthy" {
            "projection is healthy"
        } else {
            "projection is not healthy"
        },
        "inspect projection drift, re-project, capture, or clean orphaned metadata",
        json!({"instance_id": projection.instance_id, "health": projection.health}),
    ));
    checks.push(check(
        "projection",
        &format!("projection_observed_drift:{}", projection.instance_id),
        !projection.observed_drift.unwrap_or(false),
        "warning",
        "projection has no observed drift",
        "capture or re-project the skill",
        json!({"instance_id": projection.instance_id, "observed_drift": projection.observed_drift}),
    ));
    let binding_ok = projection
        .binding_id
        .as_deref()
        .is_some_and(|id| snapshot.binding(id).is_some());
    let orphan_ok = projection.binding_id.is_none() && projection.health == "orphaned";
    checks.push(check(
        "projection",
        &format!("projection_binding_exists:{}", projection.instance_id),
        binding_ok || orphan_ok,
        if orphan_ok { "warning" } else { "error" },
        if binding_ok {
            "projection binding exists"
        } else if orphan_ok {
            "projection is orphaned"
        } else {
            "projection binding is missing"
        },
        "recreate the binding or clean orphaned projection metadata",
        json!({"instance_id": projection.instance_id, "binding_id": projection.binding_id}),
    ));
    if projection.method == "symlink" {
        checks.push(check_symlink_target(ctx, projection, materialized));
    }
}

fn check_symlink_target(
    ctx: &AppContext,
    projection: &RegistryProjectionInstance,
    materialized: &Path,
) -> Value {
    let expected = ctx.skill_path(&projection.skill_id);
    let result = fs::read_link(materialized).map(|target| {
        let resolved = if target.is_absolute() {
            target
        } else {
            materialized
                .parent()
                .map(|parent| parent.join(&target))
                .unwrap_or(target)
        };
        resolved.exists() && fs::canonicalize(&resolved).ok() == fs::canonicalize(&expected).ok()
    });
    check(
        "projection",
        &format!("projection_symlink_target:{}", projection.instance_id),
        result.unwrap_or(false),
        "error",
        "symlink projection points at source skill",
        "rerun loom skill project with a supported method",
        json!({"instance_id": projection.instance_id, "path": projection.materialized_path}),
    )
}

fn check(
    section: &str,
    id: &str,
    ok: bool,
    failure_severity: &str,
    message: &str,
    next_action: &str,
    details: Value,
) -> Value {
    json!({
        "section": section,
        "id": id,
        "ok": ok,
        "severity": if ok { "ok" } else { failure_severity },
        "message": message,
        "next_action": if ok { Value::Null } else { Value::String(next_action.to_string()) },
        "details": details
    })
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
        || snapshot
            .operations
            .iter()
            .any(|op| registry_operation_mentions_skill(op, skill))
}

fn value_mentions_skill(value: &Value, skill: &str) -> bool {
    value.get("skill").and_then(Value::as_str) == Some(skill)
        || value.get("skill_id").and_then(Value::as_str) == Some(skill)
        || value
            .get("skills")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(skill)))
}

fn pending_op_mentions_skill(op: &PendingOp, skill: &str) -> bool {
    value_mentions_skill(&op.details, skill)
}

fn checks_with_severity(checks: &[Value], severity: &str) -> usize {
    checks
        .iter()
        .filter(|check| check["severity"].as_str() == Some(severity))
        .count()
}

fn drifted_path_count(checks: &[Value]) -> usize {
    checks
        .iter()
        .find(|check| check["id"].as_str() == Some("source_drift"))
        .and_then(|check| {
            check["details"]["drifted_path_count"]
                .as_u64()
                .map(|count| count as usize)
                .or_else(|| check["details"]["drifted_paths"].as_array().map(Vec::len))
        })
        .unwrap_or(0)
}
