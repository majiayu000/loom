use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::cli::{CaptureArgs, ProjectionMethod};
use crate::state::remove_path_if_exists;
use crate::state_model::{
    RegistryBindingRule, RegistryObservationEvent, RegistryOperationRecord,
    RegistryProjectionInstance, RegistryProjectionsFile, RegistryRulesFile, RegistrySnapshot,
    RegistryStatePaths,
};
use crate::types::ErrorCode;

use crate::commands::CommandFailure;
use crate::commands::file_ops::{
    copy_dir_recursive, copy_dir_recursive_preserving_symlinks, create_symlink_dir,
};

use super::rollback::{maybe_projection_fault, rollback_record_registry_operation};

// ---------------------------------------------------------------------------
// Registry state mutators
// ---------------------------------------------------------------------------

pub(crate) fn upsert_rule(rules: &mut RegistryRulesFile, rule: RegistryBindingRule) {
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
    projections: &mut RegistryProjectionsFile,
    projection: RegistryProjectionInstance,
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
        ProjectionMethod::Copy => {
            let parent = dst
                .parent()
                .context("projection target has no parent directory")?;
            let tmp_dir = parent.join(format!(".loom-tmp-{}", Uuid::new_v4()));
            if let Err(err) = copy_dir_recursive_preserving_symlinks(src, &tmp_dir) {
                let _ = remove_path_if_exists(&tmp_dir);
                return Err(err);
            }
            if let Err(err) = std::fs::rename(&tmp_dir, dst) {
                let _ = remove_path_if_exists(&tmp_dir);
                return Err(err).context("failed to atomically place projection");
            }
            Ok(())
        }
        ProjectionMethod::Materialize => {
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
    snapshot: &RegistrySnapshot,
    args: &CaptureArgs,
) -> std::result::Result<RegistryProjectionInstance, CommandFailure> {
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
        if let Some(expected_binding) = args.binding.as_deref()
            && projection.binding_id.as_deref() != Some(expected_binding)
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "instance '{}' belongs to binding '{}' not '{}'",
                    instance_id,
                    projection.binding_id.as_deref().unwrap_or("(orphaned)"),
                    expected_binding
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
        .filter(|projection| {
            projection.skill_id == skill && projection.binding_id.as_deref() == Some(binding_id)
        })
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
    projections: &mut RegistryProjectionsFile,
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

pub(crate) fn record_registry_operation(
    paths: &RegistryStatePaths,
    intent: &str,
    payload: serde_json::Value,
    effects: serde_json::Value,
) -> Result<String> {
    let op_id = format!("op_{}", Uuid::new_v4().simple());
    let now = Utc::now();
    let record = RegistryOperationRecord {
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
    let operations_len = fs::metadata(&paths.operations_file)
        .with_context(|| {
            format!(
                "failed to stat operations log {} before append",
                paths.operations_file.display()
            )
        })?
        .len();
    let checkpoint_backup = fs::read(&paths.checkpoint_file).with_context(|| {
        format!(
            "failed to snapshot checkpoint {} before operation append",
            paths.checkpoint_file.display()
        )
    })?;

    let persist_result: Result<()> = (|| -> Result<()> {
        paths.append_operation(&record)?;
        maybe_projection_fault("record_v3_operation_after_append")?;

        let mut checkpoint = paths.load_checkpoint()?;
        checkpoint.last_scanned_op_id = Some(op_id.clone());
        checkpoint.updated_at = now;
        paths.save_checkpoint(&checkpoint)?;
        maybe_projection_fault("record_v3_operation_after_checkpoint")?;
        Ok(())
    })();

    if let Err(err) = persist_result {
        if let Err(rollback_err) =
            rollback_record_registry_operation(paths, operations_len, &checkpoint_backup)
        {
            return Err(err.context(format!(
                "failed to rollback registry operation record after partial write: {}",
                rollback_err
            )));
        }
        return Err(err);
    }

    Ok(op_id)
}

pub(crate) fn record_registry_observation(
    paths: &RegistryStatePaths,
    instance_id: &str,
    kind: &str,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<String> {
    let event_id = Uuid::new_v4().to_string();
    let event = RegistryObservationEvent {
        event_id: event_id.clone(),
        instance_id: instance_id.to_string(),
        kind: kind.to_string(),
        path,
        from,
        to,
        observed_at: Utc::now(),
    };
    paths.append_observation(&event)?;
    Ok(event_id)
}

#[derive(Debug, Clone)]
pub(crate) struct RegistryAuditStateBackup {
    pub(crate) operations: Vec<u8>,
    pub(crate) checkpoint: Vec<u8>,
    pub(crate) observations: Vec<(String, Vec<u8>)>,
}

pub(crate) fn snapshot_registry_audit_state(
    paths: &RegistryStatePaths,
) -> Result<RegistryAuditStateBackup> {
    let mut observations = Vec::new();
    if paths.observations_dir.exists() {
        for entry in fs::read_dir(&paths.observations_dir).with_context(|| {
            format!(
                "failed to read observations dir {}",
                paths.observations_dir.display()
            )
        })? {
            let entry = entry.with_context(|| {
                format!(
                    "failed to read observations entry under {}",
                    paths.observations_dir.display()
                )
            })?;
            let file_type = entry.file_type().with_context(|| {
                format!(
                    "failed to inspect observation entry {}",
                    entry.path().display()
                )
            })?;
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let contents = fs::read(entry.path()).with_context(|| {
                format!("failed to snapshot observation {}", entry.path().display())
            })?;
            observations.push((name, contents));
        }
    }

    Ok(RegistryAuditStateBackup {
        operations: fs::read(&paths.operations_file)
            .with_context(|| format!("failed to snapshot {}", paths.operations_file.display()))?,
        checkpoint: fs::read(&paths.checkpoint_file)
            .with_context(|| format!("failed to snapshot {}", paths.checkpoint_file.display()))?,
        observations,
    })
}

pub(crate) fn restore_registry_audit_state(
    paths: &RegistryStatePaths,
    backup: &RegistryAuditStateBackup,
) -> Result<()> {
    fs::write(&paths.operations_file, &backup.operations)
        .with_context(|| format!("failed to restore {}", paths.operations_file.display()))?;
    fs::write(&paths.checkpoint_file, &backup.checkpoint)
        .with_context(|| format!("failed to restore {}", paths.checkpoint_file.display()))?;

    fs::create_dir_all(&paths.observations_dir).with_context(|| {
        format!(
            "failed to create observations dir {}",
            paths.observations_dir.display()
        )
    })?;
    for entry in fs::read_dir(&paths.observations_dir).with_context(|| {
        format!(
            "failed to read observations dir {}",
            paths.observations_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "failed to read observations entry under {}",
                paths.observations_dir.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect observation entry {}", path.display()))?;
        if file_type.is_dir() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("failed to remove observation dir {}", path.display()))?;
        } else {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove observation file {}", path.display()))?;
        }
    }
    for (name, contents) in &backup.observations {
        let path = paths.observations_dir.join(name);
        fs::write(&path, contents)
            .with_context(|| format!("failed to restore observation {}", path.display()))?;
    }
    Ok(())
}

