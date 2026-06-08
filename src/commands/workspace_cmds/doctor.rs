use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::envelope::Meta;
use crate::gitops;
use crate::state::{AppContext, home_dir};
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};

use super::super::helpers::{agent_kind_as_str, map_git, map_io, map_registry_state};
use super::super::{App, CommandFailure};
use super::shared::{DEFAULT_SCAN_AGENTS, default_skill_dir};

impl App {
    pub fn cmd_doctor(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let fsck = gitops::fsck(&self.ctx);
        let fsck_ok = fsck.is_ok();
        let fsck_output = fsck.unwrap_or_else(|e| e.to_string());
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let registry_paths = RegistryStatePaths::from_app_context(&self.ctx);
        let registry_schema_ok = registry_paths.schema_file.exists();
        let registry_snapshot = registry_paths
            .maybe_load_snapshot()
            .map_err(map_registry_state)?;
        let registry_snapshot_ok = registry_snapshot.is_some();
        let history = gitops::history_status(&self.ctx).map_err(map_git)?;

        let projection_checks = registry_snapshot
            .as_ref()
            .map(|snapshot| check_projection_drift(&self.ctx, snapshot))
            .unwrap_or_default();
        let projections_ok = projection_checks
            .iter()
            .all(|check| check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));

        let home_opt = home_dir();
        let agent_inventory =
            build_agent_skill_inventory(home_opt.as_deref(), registry_snapshot.as_ref());
        let agent_inventory_message = agent_inventory["message"]
            .as_str()
            .unwrap_or("agent skill directory inventory")
            .to_string();

        let mut checks_v1 = build_doctor_checks(
            &self.ctx,
            fsck_ok,
            registry_schema_ok,
            registry_snapshot.as_ref(),
            history.conflicts.is_empty(),
            pending_report.warnings.as_slice(),
        );
        checks_v1.push(json!({
            "section": "agents",
            "id": "agent_skill_inventory",
            "ok": true,
            "severity": "info",
            "message": agent_inventory_message,
            "next_action": Value::Null,
            "details": agent_inventory.clone(),
        }));
        let checks_v1_ok = checks_v1
            .iter()
            .all(|check| check.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));

        let healthy = fsck_ok
            && registry_schema_ok
            && registry_snapshot_ok
            && history.conflicts.is_empty()
            && projections_ok
            && checks_v1_ok;

        Ok((
            json!({
                "healthy": healthy,
                "checks": {
                    "git_fsck": {"ok": fsck_ok, "output": fsck_output},
                    "registry_schema_file": {"ok": registry_schema_ok},
                    "registry_snapshot": {"ok": registry_snapshot_ok},
                    "pending_queue": {
                        "count": pending_report.ops.len(),
                        "journal_events": pending_report.journal_events,
                        "history_events": pending_report.history_events,
                        "warnings": pending_report.warnings
                    },
                    "history_branch": history,
                    "projection_drift": {
                        "ok": projections_ok,
                        "projections": projection_checks
                    },
                    "agent_skill_dirs": agent_inventory
                },
                "checks_v1": checks_v1
            }),
            Meta::default(),
        ))
    }
}

fn build_agent_skill_inventory(home: Option<&Path>, snapshot: Option<&RegistrySnapshot>) -> Value {
    let mut agents: Vec<Value> = Vec::new();
    if let Some(h) = home {
        for agent in DEFAULT_SCAN_AGENTS {
            let path = default_skill_dir(agent, h);
            let path_str = path.display().to_string();
            let present = path.is_dir();
            let registered_targets: Vec<Value> = snapshot
                .map(|s| {
                    s.targets
                        .targets
                        .iter()
                        .filter(|target| paths_equivalent(&target.path, &path))
                        .map(|target| {
                            json!({
                                "target_id": target.target_id,
                                "agent": target.agent,
                                "ownership": target.ownership,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let registered_target_count = registered_targets.len();
            agents.push(json!({
                "agent": agent_kind_as_str(agent),
                "default_path": path_str,
                "present": present,
                "registered_target_count": registered_target_count,
                "registered_targets": registered_targets,
            }));
        }
    }
    let present_count = agents
        .iter()
        .filter(|a| a["present"].as_bool().unwrap_or(false))
        .count();
    let total = agents.len();
    let message = if home.is_some() {
        format!("detected {present_count} of {total} known agent skill directories")
    } else {
        "HOME not set; agent skill directory inventory unavailable".to_string()
    };
    json!({
        "agents": agents,
        "home_set": home.is_some(),
        "present_count": present_count,
        "total": total,
        "message": message,
    })
}

fn paths_equivalent(left: &str, right: &Path) -> bool {
    let left_path = Path::new(left);
    if left_path == right {
        return true;
    }

    match (fs::canonicalize(left_path), fs::canonicalize(right)) {
        (Ok(left_canon), Ok(right_canon)) => left_canon == right_canon,
        _ => false,
    }
}

pub(super) fn check_projection_drift(
    ctx: &AppContext,
    snapshot: &RegistrySnapshot,
) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    for projection in &snapshot.projections.projections {
        let materialized = Path::new(&projection.materialized_path);
        let skill_src = ctx.skill_path(&projection.skill_id);
        let mut issues: Vec<&str> = Vec::new();

        if !materialized.exists() {
            issues.push("materialized_path does not exist");
        }
        if !skill_src.exists() {
            issues.push("source skill not found in registry");
        }

        if projection.method == "symlink" && materialized.exists() {
            match fs::read_link(materialized) {
                Ok(link_target) => {
                    // Relative symlink targets resolve against the symlink's parent
                    // directory, NOT the process CWD. `Path::exists` and
                    // `fs::canonicalize` both fall back to CWD for relative inputs,
                    // so a valid relative projection (e.g. `../../skills/foo`)
                    // would otherwise be reported as dangling/wrong-target.
                    let resolved = if link_target.is_absolute() {
                        link_target.clone()
                    } else {
                        materialized
                            .parent()
                            .map(|parent| parent.join(&link_target))
                            .unwrap_or_else(|| link_target.clone())
                    };
                    if !resolved.exists() {
                        issues.push("symlink target does not exist (dangling)");
                    } else {
                        let canon_link = fs::canonicalize(&resolved).ok();
                        let canon_src = fs::canonicalize(&skill_src).ok();
                        if canon_link != canon_src {
                            issues.push("symlink points to wrong target");
                        }
                    }
                }
                Err(_) => {
                    if materialized.exists() {
                        issues.push("expected symlink but path is not a symlink");
                    }
                }
            }
        }

        results.push(json!({
            "instance_id": projection.instance_id,
            "skill_id": projection.skill_id,
            "method": projection.method,
            "ok": issues.is_empty(),
            "issues": issues,
        }));
    }
    results
}

fn build_doctor_checks(
    ctx: &AppContext,
    fsck_ok: bool,
    registry_schema_ok: bool,
    snapshot: Option<&RegistrySnapshot>,
    history_ok: bool,
    pending_warnings: &[String],
) -> Vec<Value> {
    let mut checks = vec![
        doctor_check(
            "git",
            "git_fsck",
            fsck_ok,
            "error",
            if fsck_ok {
                "git object database is healthy"
            } else {
                "git fsck reported repository issues"
            },
            "inspect git fsck output and repair the repository before writing",
            json!({}),
        ),
        doctor_check(
            "registry",
            "schema_file",
            registry_schema_ok,
            "error",
            if registry_schema_ok {
                "registry schema file exists"
            } else {
                "registry schema file is missing"
            },
            "run loom workspace init or restore state/registry/schema.json",
            json!({}),
        ),
        doctor_check(
            "registry",
            "snapshot_load",
            snapshot.is_some(),
            "error",
            if snapshot.is_some() {
                "registry snapshot loaded"
            } else {
                "registry snapshot is unavailable"
            },
            "run loom workspace init or inspect workspace status for schema errors",
            json!({}),
        ),
        doctor_check(
            "history",
            "history_branch",
            history_ok,
            "error",
            if history_ok {
                "history branch has no conflicts"
            } else {
                "history branch has conflicts"
            },
            "run loom ops history diagnose and repair before syncing",
            json!({}),
        ),
        doctor_check(
            "pending_queue",
            "pending_queue_warnings",
            pending_warnings.is_empty(),
            "warning",
            if pending_warnings.is_empty() {
                "pending queue parsed without warnings"
            } else {
                "pending queue has malformed or ignored entries"
            },
            "inspect state/pending_ops.jsonl and repair or purge malformed queue entries",
            json!({
                "warning_count": pending_warnings.len(),
                "warnings": pending_warnings
            }),
        ),
    ];

    if let Some(snapshot) = snapshot {
        checks.extend(build_registry_integrity_checks(ctx, snapshot));
    }

    checks
}

fn build_registry_integrity_checks(ctx: &AppContext, snapshot: &RegistrySnapshot) -> Vec<Value> {
    let mut checks = Vec::new();

    for target in &snapshot.targets.targets {
        let path_exists = Path::new(&target.path).exists();
        checks.push(doctor_check(
            "targets",
            &format!("target_path_exists:{}", target.target_id),
            path_exists,
            "error",
            if path_exists {
                "target path exists"
            } else {
                "target path is missing"
            },
            "recreate the target path or remove/update the target",
            json!({
                "target_id": target.target_id,
                "agent": target.agent,
                "path": target.path,
                "ownership": target.ownership
            }),
        ));
    }

    for binding in &snapshot.bindings.bindings {
        let target = snapshot.target(&binding.default_target_id);
        checks.push(doctor_check(
            "bindings",
            &format!("binding_target_exists:{}", binding.binding_id),
            target.is_some(),
            "error",
            if target.is_some() {
                "binding default target exists"
            } else {
                "binding default target is missing"
            },
            "update the binding target or recreate the missing target",
            json!({
                "binding_id": binding.binding_id,
                "target_id": binding.default_target_id
            }),
        ));

        if let Some(target) = target {
            let agent_matches = target.agent == binding.agent;
            checks.push(doctor_check(
                "bindings",
                &format!("binding_target_agent_match:{}", binding.binding_id),
                agent_matches,
                "error",
                if agent_matches {
                    "binding and target agents match"
                } else {
                    "binding and target agents do not match"
                },
                "point the binding at a target registered for the same agent",
                json!({
                    "binding_id": binding.binding_id,
                    "binding_agent": binding.agent,
                    "target_id": target.target_id,
                    "target_agent": target.agent
                }),
            ));
        }
    }

    for projection in &snapshot.projections.projections {
        let materialized_exists = Path::new(&projection.materialized_path).exists();
        checks.push(doctor_check(
            "projections",
            &format!("projection_path_exists:{}", projection.instance_id),
            materialized_exists,
            "error",
            if materialized_exists {
                "projection path exists"
            } else {
                "projection path is missing"
            },
            "rerun loom skill project or clean the orphaned projection",
            json!({
                "instance_id": projection.instance_id,
                "skill_id": projection.skill_id,
                "target_id": projection.target_id,
                "path": projection.materialized_path
            }),
        ));

        let source_exists = ctx.skill_path(&projection.skill_id).exists();
        checks.push(doctor_check(
            "projections",
            &format!("projection_source_exists:{}", projection.instance_id),
            source_exists,
            "error",
            if source_exists {
                "projection source skill exists"
            } else {
                "projection source skill is missing"
            },
            "restore the source skill or clean the orphaned projection",
            json!({
                "instance_id": projection.instance_id,
                "skill_id": projection.skill_id
            }),
        ));
    }

    checks
}

fn doctor_check(
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
