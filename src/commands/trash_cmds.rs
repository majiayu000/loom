use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{TrashAddArgs, TrashPurgeArgs, TrashRestoreArgs};
use crate::envelope::Meta;
use crate::fs_util::{remove_path_if_exists, rename_no_replace_atomic};
use crate::gitops;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::file_ops::{
    backup_path_if_exists, restore_path_from_backup, restore_path_from_backup_if_absent,
};
use super::helpers::{
    ensure_skill_exists, map_arg, map_git, map_io, map_lock, map_registry_state, slugify,
    validate_skill_name,
};
use super::projections::{
    RegistryAuditStateBackup, maybe_autosync_or_queue, record_registry_operation,
    restore_registry_audit_state, snapshot_registry_audit_state,
};
use super::{App, CommandFailure};

mod activation;
mod add;

const TRASH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrashMetadata {
    schema_version: u32,
    trash_id: String,
    skill: String,
    original_path: String,
    trashed_at: DateTime<Utc>,
    source_commit: String,
}

#[derive(Debug, Clone)]
struct TrashEntry {
    metadata: TrashMetadata,
    entry_path: PathBuf,
}

impl App {
    pub fn cmd_skill_trash_add(
        &self,
        args: &TrashAddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        add::run(self, args, request_id)
    }

    pub fn cmd_skill_trash_add_plan(
        &self,
        args: &TrashAddArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        add::plan(self, args)
    }

    pub fn cmd_skill_trash_list(
        &self,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let mut warnings = Vec::new();
        let mut entries = list_trash_entries(&self.ctx.root, &mut warnings).map_err(map_io)?;
        entries.sort_by(|a, b| {
            b.metadata
                .trashed_at
                .cmp(&a.metadata.trashed_at)
                .then_with(|| b.metadata.trash_id.cmp(&a.metadata.trash_id))
        });

        let meta = Meta {
            warnings,
            ..Meta::default()
        };
        let items = entries
            .into_iter()
            .map(|entry| {
                json!({
                    "trash_id": entry.metadata.trash_id,
                    "skill": entry.metadata.skill,
                    "original_path": entry.metadata.original_path,
                    "trashed_at": entry.metadata.trashed_at,
                    "source_commit": entry.metadata.source_commit,
                    "trash_path": entry.entry_path.strip_prefix(&self.ctx.root)
                        .unwrap_or(entry.entry_path.as_path())
                        .display()
                        .to_string()
                })
            })
            .collect::<Vec<_>>();

        Ok((json!({"items": items}), meta))
    }

    pub fn cmd_skill_trash_restore(
        &self,
        args: &TrashRestoreArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let skill_rel = format!("skills/{}", args.skill);
        let skill_path = self.ctx.root.join(&skill_rel);
        if skill_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("skill '{}' already exists", args.skill),
            ));
        }

        let entry = self.resolve_trash_entry(&args.skill, args.trash_id.as_deref())?;
        let trash_id = entry.metadata.trash_id.clone();
        let trash_rel = format!("trash/{}", trash_id);
        let trash_skill_path = entry.entry_path.join("skill");
        if !trash_skill_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("trash entry '{}' has no skill payload", trash_id),
            ));
        }

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        paths.ensure_layout().map_err(map_registry_state)?;
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let trash_backup = backup_path_if_exists(&self.ctx, &entry.entry_path, "trash-restore")
            .map_err(map_registry_state)?;

        if let Err(err) = trash_test_pause("before_restore_activation")
            .and_then(|()| rename_no_replace_atomic(&trash_skill_path, &skill_path))
        {
            remove_temp_backup_best_effort(trash_backup.as_ref());
            return Err(map_io(err));
        }
        let mut metadata_capture = None;
        if let Err(err) = trash_test_pause("before_restore_cleanup").and_then(|()| {
            let backup_path = trash_backup
                .as_ref()
                .and_then(|backup| backup["backup_path"].as_str())
                .map(Path::new)
                .ok_or_else(|| std::io::Error::other("trash payload backup is missing"))?;
            let captured = backup_path.with_extension("metadata-recovery");
            // Claim the current file atomically before inspecting it. A new
            // metadata.json created afterwards prevents nonrecursive cleanup.
            rename_no_replace_atomic(&entry.entry_path.join("metadata.json"), &captured)?;
            metadata_capture = Some(captured.clone());
            trash_test_pause("after_restore_metadata_capture")?;
            if !fs::symlink_metadata(&captured)?.file_type().is_file() {
                return Err(std::io::Error::other(
                    "trash metadata type changed after snapshot; preserving captured metadata",
                ));
            }
            if fs::read(&captured)? != fs::read(backup_path.join("metadata.json"))? {
                return Err(std::io::Error::other(
                    "trash metadata changed after snapshot; preserving captured metadata",
                ));
            }
            fs::remove_dir(&entry.entry_path)
        }) {
            let rollback_errors = rollback_restore_from_backup(
                &skill_path,
                &entry.entry_path,
                trash_backup.as_ref(),
                metadata_capture.as_deref(),
            );
            return Err(map_io(err).with_rollback_errors(rollback_errors));
        }

        let op_id = match record_registry_operation(
            &paths,
            "skill.trash.restore",
            json!({
                "skill": args.skill,
                "trash_id": trash_id,
                "request_id": request_id
            }),
            json!({
                "trash_id": trash_id,
                "restored_path": skill_rel
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                let mut rollback_errors = rollback_restore_from_backup(
                    &skill_path,
                    &entry.entry_path,
                    trash_backup.as_ref(),
                    metadata_capture.as_deref(),
                );
                rollback_errors.extend(restore_registry_audit_state_best_effort(
                    &paths,
                    &registry_backup,
                ));
                unstage_trash_paths(&self.ctx, &[&skill_rel, &trash_rel]);
                return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
            }
        };

        if let Err(err) = stage_trash_commit_paths(&self.ctx, &[&skill_rel, &trash_rel]) {
            let mut rollback_errors = rollback_restore_from_backup(
                &skill_path,
                &entry.entry_path,
                trash_backup.as_ref(),
                metadata_capture.as_deref(),
            );
            rollback_errors.extend(restore_registry_audit_state_best_effort(
                &paths,
                &registry_backup,
            ));
            unstage_trash_paths(&self.ctx, &[&skill_rel, &trash_rel]);
            return Err(err.with_rollback_errors(rollback_errors));
        }

        let commit = match commit_trash_paths(
            &self.ctx,
            &[&skill_rel, &trash_rel],
            &format!("restore({}): restore from trash", args.skill),
        ) {
            Ok(commit) => commit,
            Err(err) => {
                let mut rollback_errors = rollback_restore_from_backup(
                    &skill_path,
                    &entry.entry_path,
                    trash_backup.as_ref(),
                    metadata_capture.as_deref(),
                );
                rollback_errors.extend(restore_registry_audit_state_best_effort(
                    &paths,
                    &registry_backup,
                ));
                unstage_trash_paths(&self.ctx, &[&skill_rel, &trash_rel]);
                return Err(map_git(err).with_rollback_errors(rollback_errors));
            }
        };
        if let Some(path) = metadata_capture {
            let _ = fs::remove_file(path);
        }
        remove_temp_backup_best_effort(trash_backup.as_ref());

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        maybe_autosync_or_queue(
            &self.ctx,
            "trash_restore",
            request_id,
            json!({"skill": args.skill, "trash_id": trash_id, "commit": commit}),
            &mut meta,
        )?;

        Ok((
            json!({
                "skill": args.skill,
                "trash_id": trash_id,
                "commit": commit
            }),
            meta,
        ))
    }

    pub fn cmd_skill_trash_purge(
        &self,
        args: &TrashPurgeArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_trash_id(&args.trash_id)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;

        let entry_path = self.ctx.root.join("trash").join(&args.trash_id);
        if !entry_path.exists() {
            return Err(trash_entry_not_found(&args.trash_id));
        }
        let metadata = read_trash_metadata(&entry_path).map_err(map_io)?;
        let _lock = self.ctx.lock_skill(&metadata.skill).map_err(map_lock)?;

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        paths.ensure_layout().map_err(map_registry_state)?;
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let trash_backup = backup_path_if_exists(&self.ctx, &entry_path, "trash-purge")
            .map_err(map_registry_state)?;

        if let Err(err) = trash_test_pause("before_purge_remove")
            .and_then(|()| remove_path_if_exists(&entry_path))
        {
            let rollback_errors =
                rollback_trash_payload(&entry_path, trash_backup.as_ref(), None, false);
            return Err(map_io(err).with_rollback_errors(rollback_errors));
        }

        let trash_rel = format!("trash/{}", args.trash_id);
        let op_id = match record_registry_operation(
            &paths,
            "skill.trash.purge",
            json!({
                "skill": metadata.skill,
                "trash_id": args.trash_id,
                "request_id": request_id
            }),
            json!({
                "trash_id": args.trash_id,
                "purged": true
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                let mut rollback_errors =
                    rollback_trash_payload(&entry_path, trash_backup.as_ref(), None, false);
                rollback_errors.extend(restore_registry_audit_state_best_effort(
                    &paths,
                    &registry_backup,
                ));
                unstage_trash_paths(&self.ctx, &[&trash_rel]);
                return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
            }
        };

        if let Err(err) = stage_trash_commit_paths(&self.ctx, &[&trash_rel]) {
            let mut rollback_errors =
                rollback_trash_payload(&entry_path, trash_backup.as_ref(), None, false);
            rollback_errors.extend(restore_registry_audit_state_best_effort(
                &paths,
                &registry_backup,
            ));
            unstage_trash_paths(&self.ctx, &[&trash_rel]);
            return Err(err.with_rollback_errors(rollback_errors));
        }

        let commit = match commit_trash_paths(
            &self.ctx,
            &[&trash_rel],
            &format!("purge({}): remove trash entry", args.trash_id),
        ) {
            Ok(commit) => commit,
            Err(err) => {
                let mut rollback_errors =
                    rollback_trash_payload(&entry_path, trash_backup.as_ref(), None, false);
                rollback_errors.extend(restore_registry_audit_state_best_effort(
                    &paths,
                    &registry_backup,
                ));
                unstage_trash_paths(&self.ctx, &[&trash_rel]);
                return Err(map_git(err).with_rollback_errors(rollback_errors));
            }
        };
        remove_temp_backup_best_effort(trash_backup.as_ref());

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        maybe_autosync_or_queue(
            &self.ctx,
            "trash_purge",
            request_id,
            json!({"trash_id": args.trash_id, "commit": commit}),
            &mut meta,
        )?;

        Ok((json!({"trash_id": args.trash_id, "commit": commit}), meta))
    }

    pub fn cmd_skill_trash_purge_plan(
        &self,
        args: &TrashPurgeArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_trash_id(&args.trash_id)?;
        let entry_path = self.ctx.root.join("trash").join(&args.trash_id);
        if !entry_path.exists() {
            return Err(trash_entry_not_found(&args.trash_id));
        }
        let metadata = read_trash_metadata(&entry_path).map_err(map_io)?;

        Ok((
            json!({
                "trash_id": args.trash_id,
                "skill": metadata.skill,
                "dry_run": true,
                "would_purge": true,
                "trash_path": format!("trash/{}", args.trash_id),
                "would_record_operation": true,
                "would_commit": true
            }),
            Meta::default(),
        ))
    }

    fn resolve_trash_entry(
        &self,
        skill: &str,
        trash_id: Option<&str>,
    ) -> std::result::Result<TrashEntry, CommandFailure> {
        if let Some(trash_id) = trash_id {
            validate_trash_id(trash_id)?;
            let entry_path = self.ctx.root.join("trash").join(trash_id);
            if !entry_path.exists() {
                return Err(trash_entry_not_found(trash_id));
            }
            let metadata = read_trash_metadata(&entry_path).map_err(map_io)?;
            if metadata.skill != skill {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "trash entry '{}' contains skill '{}', not '{}'",
                        trash_id, metadata.skill, skill
                    ),
                ));
            }
            return Ok(TrashEntry {
                metadata,
                entry_path,
            });
        }

        let mut warnings = Vec::new();
        let mut entries = list_trash_entries(&self.ctx.root, &mut warnings).map_err(map_io)?;
        entries.retain(|entry| entry.metadata.skill == skill);
        entries.sort_by(|a, b| {
            b.metadata
                .trashed_at
                .cmp(&a.metadata.trashed_at)
                .then_with(|| b.metadata.trash_id.cmp(&a.metadata.trash_id))
        });
        entries.into_iter().next().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::TrashEntryNotFound,
                format!("no trash entry found for skill '{}'", skill),
            )
        })
    }
}

fn trash_entry_not_found(trash_id: &str) -> CommandFailure {
    CommandFailure::new(
        ErrorCode::TrashEntryNotFound,
        format!("trash entry '{}' not found", trash_id),
    )
}

fn new_trash_id(skill: &str) -> String {
    let ts = Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let suffix = Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    format!("{}-{}-{}", slugify(skill), ts, suffix)
}

fn validate_trash_id(trash_id: &str) -> std::result::Result<(), CommandFailure> {
    if trash_id.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "trash id cannot be empty",
        ));
    }
    if trash_id == "." || trash_id == ".." {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "trash id cannot be '.' or '..'",
        ));
    }
    if trash_id
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "trash id '{}' contains unsupported characters; use [A-Za-z0-9._-]",
                trash_id
            ),
        ));
    }
    Ok(())
}

fn write_trash_metadata(entry_path: &Path, metadata: &TrashMetadata) -> Result<()> {
    let raw = serde_json::to_string_pretty(metadata)? + "\n";
    fs::write(entry_path.join("metadata.json"), raw).with_context(|| {
        format!(
            "failed to write trash metadata under {}",
            entry_path.display()
        )
    })
}

fn read_trash_metadata(entry_path: &Path) -> Result<TrashMetadata> {
    let metadata_path = entry_path.join("metadata.json");
    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let metadata: TrashMetadata = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;
    if metadata.schema_version != TRASH_SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported trash schema version {} in {}",
            metadata.schema_version,
            metadata_path.display()
        ));
    }
    validate_trash_id(&metadata.trash_id).map_err(|err| anyhow!(err.message))?;
    let directory_id = entry_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("trash entry path has no valid directory name"))?;
    if metadata.trash_id != directory_id {
        return Err(anyhow!(
            "trash metadata id '{}' does not match directory '{}'",
            metadata.trash_id,
            directory_id
        ));
    }
    validate_skill_name(&metadata.skill)?;
    Ok(metadata)
}

fn list_trash_entries(root: &Path, warnings: &mut Vec<String>) -> Result<Vec<TrashEntry>> {
    let trash_dir = root.join("trash");
    if !trash_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&trash_dir)
        .with_context(|| format!("failed to read trash dir {}", trash_dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry under {}", trash_dir.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        match read_trash_metadata(&entry.path()) {
            Ok(metadata) => entries.push(TrashEntry {
                metadata,
                entry_path: entry.path(),
            }),
            Err(err) => warnings.push(format!(
                "skipping malformed trash entry {}: {}",
                entry.path().display(),
                err
            )),
        }
    }
    Ok(entries)
}

fn stage_trash_commit_paths(
    ctx: &crate::state::AppContext,
    paths: &[&str],
) -> std::result::Result<(), CommandFailure> {
    for path in paths {
        if gitops::path_exists_or_is_tracked(ctx, path).map_err(map_git)? {
            gitops::run_git(ctx, &["add", "-A", "--", path]).map_err(map_git)?;
        }
    }
    gitops::run_git(ctx, &["add", "-A", "--", "state/registry"]).map_err(map_git)?;
    let legacy_v3_tracked =
        gitops::run_git_allow_failure(ctx, &["ls-files", "--error-unmatch", "--", "state/v3"])
            .map_err(map_git)?
            .status
            .success();
    if ctx.state_dir.join("v3").exists() || legacy_v3_tracked {
        gitops::run_git(ctx, &["add", "-A", "--", "state/v3"]).map_err(map_git)?;
    }
    Ok(())
}

fn commit_trash_paths(
    ctx: &crate::state::AppContext,
    paths: &[&str],
    message: &str,
) -> Result<String> {
    let mut commit_paths = paths
        .iter()
        .filter_map(|path| match trash_path_should_be_committed(ctx, path) {
            Ok(true) => Some(Ok((*path).to_string())),
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<Result<Vec<_>>>()?;
    commit_paths.push("state/registry".to_string());
    let legacy_v3_tracked =
        gitops::run_git_allow_failure(ctx, &["ls-files", "--error-unmatch", "--", "state/v3"])?
            .status
            .success();
    if ctx.state_dir.join("v3").exists() || legacy_v3_tracked {
        commit_paths.push("state/v3".to_string());
    }

    let mut args = vec![
        "commit".to_string(),
        "-m".to_string(),
        message.to_string(),
        "--".to_string(),
    ];
    args.extend(commit_paths);
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    gitops::run_git(ctx, &refs)?;
    gitops::head(ctx)
}

fn trash_path_should_be_committed(ctx: &crate::state::AppContext, path: &str) -> Result<bool> {
    if gitops::path_exists_or_is_tracked(ctx, path)? {
        return Ok(true);
    }
    gitops::has_staged_changes_for_path(ctx, Path::new(path))
}

fn unstage_trash_paths(ctx: &crate::state::AppContext, paths: &[&str]) {
    for path in paths {
        let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", path]);
    }
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/registry"]);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/v3"]);
}

fn rollback_restore_from_backup(
    skill_path: &Path,
    entry_path: &Path,
    backup: Option<&Value>,
    metadata_capture: Option<&Path>,
) -> Vec<Value> {
    let mut errors = rollback_trash_payload(
        entry_path,
        backup,
        Some(skill_path),
        metadata_capture.is_some(),
    );
    if let Some(path) = metadata_capture {
        errors.push(trash_rollback_error(
            "preserve_trash_metadata",
            path,
            backup,
            anyhow!("preserving captured trash metadata for manual recovery"),
        ));
    }
    errors
}

fn rollback_trash_payload(
    entry_path: &Path,
    backup: Option<&Value>,
    restored_skill_path: Option<&Path>,
    preserve_backup: bool,
) -> Vec<Value> {
    let mut errors = Vec::new();
    let restore = || -> Result<()> {
        if std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref()
            == Some("restore_trash_payload")
        {
            return Err(anyhow!("fault injected at restore_trash_payload"));
        }
        let backup = backup.ok_or_else(|| anyhow!("trash payload backup is missing"))?;
        trash_test_pause("before_rollback_restore")?;
        let candidate =
            entry_path.with_file_name(format!(".loom-trash-recovery-{}", Uuid::new_v4()));
        restore_path_from_backup_if_absent(entry_path, &candidate, backup)
    };
    if let Err(err) = restore() {
        errors.push(trash_rollback_error(
            "restore_trash_payload",
            entry_path,
            backup,
            err,
        ));
        return errors;
    }

    // Live data can have changed since the snapshot. Keep it and the backup
    // for manual recovery rather than deleting an existing path during rollback.
    if let Some(skill_path) = restored_skill_path {
        match fs::symlink_metadata(skill_path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            result => {
                let err = match result {
                    Ok(_) => anyhow!("preserving existing live skill for manual recovery"),
                    Err(err) => anyhow::Error::from(err),
                };
                errors.push(trash_rollback_error(
                    "remove_restored_skill",
                    skill_path,
                    backup,
                    err,
                ));
                return errors;
            }
        }
    }
    if !preserve_backup {
        remove_temp_backup_best_effort(backup);
    }
    errors
}

fn trash_rollback_error(
    step: &str,
    path: &Path,
    backup: Option<&Value>,
    err: anyhow::Error,
) -> Value {
    json!({
        "step": step,
        "message": format!("{err:#}"),
        "path": path.display().to_string(),
        "backup_path": backup.and_then(|backup| backup.get("backup_path")),
    })
}

fn remove_temp_backup_best_effort(backup: Option<&serde_json::Value>) {
    let Some(path) = backup
        .and_then(|backup| backup.get("backup_path"))
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
    else {
        return;
    };
    let _ = remove_path_if_exists(path);
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent);
        if let Some(grandparent) = parent.parent() {
            let _ = fs::remove_dir(grandparent);
        }
    }
}

fn restore_registry_audit_state_best_effort(
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
) -> Vec<Value> {
    let step = "restore_registry_audit_state";
    if std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref() == Some(step) {
        return vec![json!({"step": step, "message": format!("fault injected at {}", step)})];
    }
    restore_registry_audit_state(paths, registry_backup)
        .err()
        .map(|err| vec![json!({"step": step, "message": err.to_string()})])
        .unwrap_or_default()
}

#[cfg(debug_assertions)]
fn trash_test_pause(point: &str) -> std::io::Result<()> {
    if std::env::var("LOOM_TEST_TRASH_PAUSE_POINT").ok().as_deref() == Some(point) {
        let directory = std::env::var_os("LOOM_TEST_TRASH_PAUSE_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| std::io::Error::other("trash pause directory is absent"))?;
        fs::write(directory.join("ready"), point)?;
        let mut released = false;
        for _ in 0..2_000 {
            if directory.join("release").try_exists()? {
                released = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if !released {
            return Err(std::io::Error::other("trash test pause timed out"));
        }
    }
    if std::env::var("LOOM_TEST_TRASH_FAIL_POINT").ok().as_deref() == Some(point) {
        return Err(std::io::Error::other(format!("fault injected at {point}")));
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
fn trash_test_pause(_point: &str) -> std::io::Result<()> {
    Ok(())
}
