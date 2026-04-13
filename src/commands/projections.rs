use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use crate::cli::{CaptureArgs, ProjectionMethod};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::{AppContext, PendingOpsReport, resolve_agent_skill_source_dirs};
use crate::state_model::{
    V3BindingRule, V3OperationRecord, V3ProjectionInstance, V3ProjectionsFile, V3RulesFile,
    V3Snapshot, V3StatePaths,
};
use crate::types::{ErrorCode, SyncState};

use super::CommandFailure;
use super::helpers::{
    map_git, map_io, map_push_rejected, map_queue, map_remote_unreachable,
};
use super::file_ops::{copy_dir_recursive, create_symlink_dir};
use crate::state::remove_path_if_exists;

// ---------------------------------------------------------------------------
// V3 state mutators
// ---------------------------------------------------------------------------

pub(crate) fn upsert_rule(rules: &mut V3RulesFile, rule: V3BindingRule) {
    if let Some(existing) = rules.rules.iter_mut().find(|existing| {
        existing.binding_id == rule.binding_id
            && existing.skill_id == rule.skill_id
            && existing.target_id == rule.target_id
    }) {
        existing.method = rule.method;
        existing.watch_policy = rule.watch_policy;
        return;
    }

    rules.rules.push(rule);
    rules.rules.sort_by(|left, right| {
        left.binding_id
            .cmp(&right.binding_id)
            .then_with(|| left.skill_id.cmp(&right.skill_id))
            .then_with(|| left.target_id.cmp(&right.target_id))
    });
}

pub(crate) fn upsert_projection(
    projections: &mut V3ProjectionsFile,
    projection: V3ProjectionInstance,
) {
    if let Some(existing) = projections
        .projections
        .iter_mut()
        .find(|existing| existing.instance_id == projection.instance_id)
    {
        *existing = projection;
        return;
    }

    projections.projections.push(projection);
    projections
        .projections
        .sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
}

pub(crate) fn project_skill_to_target(
    src: &Path,
    dst: &Path,
    method: ProjectionMethod,
) -> Result<()> {
    match method {
        ProjectionMethod::Symlink => create_symlink_dir(src, dst),
        ProjectionMethod::Copy | ProjectionMethod::Materialize => {
            let parent = dst
                .parent()
                .context("projection target has no parent directory")?;
            let tmp_dir = parent.join(format!(".loom-tmp-{}", Uuid::new_v4()));
            if let Err(err) = copy_dir_recursive(src, &tmp_dir) {
                let _ = remove_path_if_exists(&tmp_dir);
                return Err(err);
            }
            if let Err(err) = std::fs::rename(&tmp_dir, dst) {
                let _ = remove_path_if_exists(&tmp_dir);
                return Err(err).context("failed to atomically place projection");
            }
            Ok(())
        }
    }
}

pub(crate) fn resolve_capture_projection(
    snapshot: &V3Snapshot,
    args: &CaptureArgs,
) -> std::result::Result<V3ProjectionInstance, CommandFailure> {
    if let Some(instance_id) = args.instance.as_deref() {
        let projection = snapshot
            .projections
            .projections
            .iter()
            .find(|projection| projection.instance_id == instance_id)
            .cloned()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("projection instance '{}' not found", instance_id),
                )
            })?;
        if let Some(skill) = args.skill.as_deref()
            && projection.skill_id != skill
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "instance '{}' belongs to skill '{}' not '{}'",
                    instance_id, projection.skill_id, skill
                ),
            ));
        }
        if let Some(binding_id) = args.binding.as_deref()
            && projection.binding_id != binding_id
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "instance '{}' belongs to binding '{}' not '{}'",
                    instance_id, projection.binding_id, binding_id
                ),
            ));
        }
        return Ok(projection);
    }

    let skill = args.skill.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            "capture requires <skill> or --instance",
        )
    })?;
    let binding_id = args.binding.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            "capture requires --binding when --instance is not provided",
        )
    })?;

    let matches = snapshot
        .projections
        .projections
        .iter()
        .filter(|projection| projection.skill_id == skill && projection.binding_id == binding_id)
        .cloned()
        .collect::<Vec<_>>();

    match matches.len() {
        0 => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "no projection found for skill '{}' and binding '{}'",
                skill, binding_id
            ),
        )),
        1 => Ok(matches.into_iter().next().expect("single projection")),
        _ => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "multiple projections found for skill '{}' and binding '{}'; use --instance",
                skill, binding_id
            ),
        )),
    }
}

pub(crate) fn update_projection_after_capture(
    projections: &mut V3ProjectionsFile,
    instance_id: &str,
    rev: &str,
) -> std::result::Result<(), CommandFailure> {
    let projection = projections
        .projections
        .iter_mut()
        .find(|projection| projection.instance_id == instance_id)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "projection instance '{}' not found during capture update",
                    instance_id
                ),
            )
        })?;
    projection.last_applied_rev = rev.to_string();
    projection.health = "healthy".to_string();
    projection.observed_drift = Some(false);
    projection.updated_at = Some(Utc::now());
    Ok(())
}

pub(crate) fn record_v3_operation(
    paths: &V3StatePaths,
    intent: &str,
    payload: serde_json::Value,
    effects: serde_json::Value,
) -> Result<String> {
    let op_id = format!("op_{}", Uuid::new_v4().simple());
    let now = Utc::now();
    let record = V3OperationRecord {
        op_id: op_id.clone(),
        intent: intent.to_string(),
        status: "succeeded".to_string(),
        ack: false,
        payload,
        effects,
        last_error: None,
        created_at: now,
        updated_at: now,
    };
    paths.append_operation(&record)?;

    let mut checkpoint = paths.load_checkpoint()?;
    checkpoint.last_scanned_op_id = Some(op_id.clone());
    checkpoint.updated_at = now;
    paths.save_checkpoint(&checkpoint)?;
    Ok(op_id)
}

// ---------------------------------------------------------------------------
// SkillInventory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SkillInventory {
    pub source_skills: Vec<String>,
    pub backup_skills: Vec<String>,
    pub source_dirs: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub fn collect_skill_inventory(ctx: &AppContext) -> SkillInventory {
    let source_dirs = resolve_agent_skill_source_dirs(&ctx.root);
    let mut warnings = Vec::new();

    let source_skills = list_unique_skills_from_dirs(&source_dirs, "source", &mut warnings);
    let backup_skills = list_unique_skills_from_dirs(
        std::slice::from_ref(&ctx.skills_dir),
        "backup",
        &mut warnings,
    );

    SkillInventory {
        source_skills,
        backup_skills,
        source_dirs,
        warnings,
    }
}

fn list_unique_skills_from_dirs(
    dirs: &[PathBuf],
    label: &str,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut skills = BTreeSet::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(format!(
                    "failed to read {} skills dir {}: {}",
                    label,
                    dir.display(),
                    err
                ));
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!(
                        "failed to read entry in {} skills dir {}: {}",
                        label,
                        dir.display(),
                        err
                    ));
                    continue;
                }
            };

            let is_dir = match entry.file_type() {
                Ok(kind) if kind.is_dir() => true,
                Ok(kind) if kind.is_symlink() => fs::metadata(entry.path())
                    .map(|meta| meta.is_dir())
                    .unwrap_or(false),
                Ok(_) => false,
                Err(err) => {
                    warnings.push(format!(
                        "failed to inspect entry {} in {} skills dir {}: {}",
                        entry.file_name().to_string_lossy(),
                        label,
                        dir.display(),
                        err
                    ));
                    false
                }
            };

            if is_dir {
                skills.insert(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    skills.into_iter().collect()
}

// ---------------------------------------------------------------------------
// Remote status / sync internals
// ---------------------------------------------------------------------------

pub fn remote_status_payload(
    ctx: &AppContext,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let pending_report = ctx.read_pending_report().map_err(map_io)?;
    remote_status_payload_with_pending(ctx, pending_report)
}

pub(crate) fn remote_status_payload_with_pending(
    ctx: &AppContext,
    pending_report: PendingOpsReport,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let pending = pending_report.ops.len();

    if !gitops::remote_exists(ctx) {
        return Ok((
            json!({
                "configured": false,
                "pending_ops": pending,
                "sync_state": SyncState::LocalOnly,
            }),
            Meta {
                warnings: pending_report
                    .warnings
                    .into_iter()
                    .chain(std::iter::once("remote origin not configured".to_string()))
                    .collect(),
                sync_state: Some(SyncState::LocalOnly),
                op_id: None,
            },
        ));
    }

    let url = gitops::remote_url(ctx)
        .map_err(map_git)?
        .unwrap_or_default();
    let mut meta = Meta {
        warnings: pending_report.warnings,
        sync_state: None,
        op_id: None,
    };

    if !gitops::remote_tracking_main_exists(ctx).map_err(map_git)? {
        let sync_state = if pending > 0 {
            SyncState::PendingPush
        } else {
            SyncState::LocalOnly
        };
        meta.warnings.push(
            "origin/main has not been fetched yet; status is based on local state".to_string(),
        );
        meta.sync_state = Some(sync_state.clone());
        return Ok((
            json!({
                "configured": true,
                "remote": "origin",
                "url": url,
                "pending_ops": pending,
                "tracking_ref": false,
                "sync_state": sync_state,
            }),
            meta,
        ));
    }

    let (ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
    let sync_state = if pending > 0 {
        SyncState::PendingPush
    } else if ahead == 0 && behind == 0 {
        SyncState::Synced
    } else if ahead > 0 && behind == 0 {
        SyncState::PendingPush
    } else {
        SyncState::Diverged
    };
    meta.sync_state = Some(sync_state.clone());

    Ok((
        json!({
            "configured": true,
            "remote": "origin",
            "url": url,
            "ahead": ahead,
            "behind": behind,
            "pending_ops": pending,
            "tracking_ref": true,
            "sync_state": sync_state,
        }),
        meta,
    ))
}

pub(crate) fn maybe_autosync_or_queue(
    ctx: &AppContext,
    command: &str,
    request_id: &str,
    details: serde_json::Value,
    meta: &mut Meta,
) -> std::result::Result<(), CommandFailure> {
    if !gitops::remote_exists(ctx) {
        ctx.append_pending(command, details, request_id.to_string())
            .map_err(map_queue)?;
        meta.sync_state = Some(SyncState::PendingPush);
        meta.warnings
            .push("remote origin not configured, operation queued".to_string());
        return Ok(());
    }

    match sync_push_internal(ctx) {
        Ok(_) => {
            meta.sync_state = Some(SyncState::Synced);
        }
        Err(err) => {
            ctx.append_pending(command, details, request_id.to_string())
                .map_err(map_queue)?;
            meta.sync_state = Some(match err.code {
                ErrorCode::RemoteDiverged => SyncState::Diverged,
                ErrorCode::ReplayConflict => SyncState::Conflicted,
                _ => SyncState::PendingPush,
            });
            meta.warnings.push(format!(
                "auto sync failed ({}), operation queued",
                err.code.as_str()
            ));
        }
    }
    Ok(())
}

pub(crate) fn sync_push_internal(
    ctx: &AppContext,
) -> std::result::Result<&'static str, CommandFailure> {
    if !gitops::remote_exists(ctx) {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "remote origin not configured",
        ));
    }

    let pending_report = ctx.read_pending_report().map_err(map_io)?;
    let queued_ids = pending_report
        .ops
        .iter()
        .map(|op| op.stable_id())
        .collect::<std::collections::BTreeSet<_>>();
    let remote_main_exists =
        gitops::fetch_origin_main_if_present(ctx).map_err(map_remote_unreachable)?;
    let remote_history_exists =
        gitops::fetch_origin_history_branch_if_present(ctx).map_err(map_remote_unreachable)?;
    if remote_history_exists {
        let _ = gitops::sync_history_branch_from_remote(ctx).map_err(map_git)?;
    }
    if remote_main_exists {
        let (_ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
        if behind > 0 {
            return Err(CommandFailure::new(
                ErrorCode::RemoteDiverged,
                "local branch is behind origin/main",
            ));
        }
    }
    gitops::push_main_with_tags(ctx).map_err(map_push_rejected)?;
    ctx.remove_pending_ops(&queued_ids).map_err(map_queue)?;
    Ok("pushed")
}

pub(crate) fn sync_replay_internal(
    ctx: &AppContext,
) -> std::result::Result<&'static str, CommandFailure> {
    let pending = ctx.pending_count().map_err(map_io)?;
    if pending == 0 {
        return Ok("no_pending_ops");
    }
    sync_push_internal(ctx)?;
    Ok("replayed")
}
