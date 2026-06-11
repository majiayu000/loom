use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{TrashAddArgs, TrashPurgeArgs, TrashRestoreArgs};
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::gitops;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::file_ops::{backup_path_if_exists, restore_path_from_backup};
use super::helpers::{
    map_arg, map_git, map_io, map_lock, map_registry_state, slugify, validate_skill_name,
};
use super::projections::{
    RegistryAuditStateBackup, maybe_autosync_or_queue, record_registry_operation,
    restore_registry_audit_state, snapshot_registry_audit_state,
};
use super::{App, CommandFailure};

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
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let skill_rel = format!("skills/{}", args.skill);
        let skill_path = self.ctx.root.join(&skill_rel);
        if !skill_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        paths.ensure_layout().map_err(map_registry_state)?;
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let source_commit = gitops::head(&self.ctx).map_err(map_git)?;
        let trash_id = new_trash_id(&args.skill);
        let entry_path = self.ctx.root.join("trash").join(&trash_id);
        let trash_skill_path = entry_path.join("skill");
        fs::create_dir_all(&entry_path).map_err(map_io)?;

        if let Err(err) = fs::rename(&skill_path, &trash_skill_path) {
            let _ = remove_path_if_exists(&entry_path);
            return Err(map_io(err));
        }

        let metadata = TrashMetadata {
            schema_version: TRASH_SCHEMA_VERSION,
            trash_id: trash_id.clone(),
            skill: args.skill.clone(),
            original_path: skill_rel.clone(),
            trashed_at: Utc::now(),
            source_commit: source_commit.clone(),
        };
        if let Err(err) = write_trash_metadata(&entry_path, &metadata) {
            rollback_trash_add(&skill_path, &trash_skill_path, &entry_path);
            return Err(map_io(err));
        }

        let op_id = match record_registry_operation(
            &paths,
            "skill.trash.add",
            json!({
                "skill": args.skill,
                "trash_id": trash_id,
                "request_id": request_id
            }),
            json!({
                "trash_id": trash_id,
                "trash_path": format!("trash/{}", trash_id),
                "source_commit": source_commit
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                rollback_trash_add(&skill_path, &trash_skill_path, &entry_path);
                let rollback_errors =
                    restore_registry_audit_state_best_effort(&paths, &registry_backup);
                unstage_trash_paths(&self.ctx, &[&skill_rel, &format!("trash/{}", trash_id)]);
                return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
            }
        };

        if let Err(err) =
            stage_trash_commit_paths(&self.ctx, &[&skill_rel, &format!("trash/{}", trash_id)])
        {
            rollback_trash_add(&skill_path, &trash_skill_path, &entry_path);
            let rollback_errors =
                restore_registry_audit_state_best_effort(&paths, &registry_backup);
            unstage_trash_paths(&self.ctx, &[&skill_rel, &format!("trash/{}", trash_id)]);
            return Err(err.with_rollback_errors(rollback_errors));
        }

        let commit = match commit_trash_paths(
            &self.ctx,
            &[&skill_rel, &format!("trash/{}", trash_id)],
            &format!("trash({}): move to trash", args.skill),
        ) {
            Ok(commit) => commit,
            Err(err) => {
                rollback_trash_add(&skill_path, &trash_skill_path, &entry_path);
                let rollback_errors =
                    restore_registry_audit_state_best_effort(&paths, &registry_backup);
                unstage_trash_paths(&self.ctx, &[&skill_rel, &format!("trash/{}", trash_id)]);
                return Err(map_git(err).with_rollback_errors(rollback_errors));
            }
        };

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        maybe_autosync_or_queue(
            &self.ctx,
            "trash_add",
            request_id,
            json!({"skill": args.skill, "trash_id": trash_id, "commit": commit}),
            &mut meta,
        )?;

        Ok((
            json!({
                "skill": args.skill,
                "trash_id": trash_id,
                "trash_path": format!("trash/{}", trash_id),
                "commit": commit
            }),
            meta,
        ))
    }

    pub fn cmd_skill_trash_add_plan(
        &self,
        args: &TrashAddArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let skill_rel = format!("skills/{}", args.skill);
        let skill_path = self.ctx.root.join(&skill_rel);
        if !skill_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        Ok((
            json!({
                "skill": args.skill,
                "dry_run": true,
                "would_move": true,
                "original_path": skill_rel,
                "trash_path": format!("trash/{}", new_trash_id(&args.skill)),
                "would_record_operation": true,
                "would_commit": true
            }),
            Meta::default(),
        ))
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

        if let Err(err) = fs::rename(&trash_skill_path, &skill_path) {
            remove_temp_backup_best_effort(trash_backup.as_ref());
            return Err(map_io(err));
        }
        if let Err(err) = remove_path_if_exists(&entry.entry_path) {
            rollback_restore_from_backup(&skill_path, &entry.entry_path, trash_backup.as_ref());
            remove_temp_backup_best_effort(trash_backup.as_ref());
            return Err(map_io(err));
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
                rollback_restore_from_backup(&skill_path, &entry.entry_path, trash_backup.as_ref());
                remove_temp_backup_best_effort(trash_backup.as_ref());
                let rollback_errors =
                    restore_registry_audit_state_best_effort(&paths, &registry_backup);
                unstage_trash_paths(&self.ctx, &[&skill_rel, &trash_rel]);
                return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
            }
        };

        if let Err(err) = stage_trash_commit_paths(&self.ctx, &[&skill_rel, &trash_rel]) {
            rollback_restore_from_backup(&skill_path, &entry.entry_path, trash_backup.as_ref());
            remove_temp_backup_best_effort(trash_backup.as_ref());
            let rollback_errors =
                restore_registry_audit_state_best_effort(&paths, &registry_backup);
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
                rollback_restore_from_backup(&skill_path, &entry.entry_path, trash_backup.as_ref());
                remove_temp_backup_best_effort(trash_backup.as_ref());
                let rollback_errors =
                    restore_registry_audit_state_best_effort(&paths, &registry_backup);
                unstage_trash_paths(&self.ctx, &[&skill_rel, &trash_rel]);
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

        if let Err(err) = remove_path_if_exists(&entry_path) {
            remove_temp_backup_best_effort(trash_backup.as_ref());
            return Err(map_io(err));
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
                restore_temp_backup_best_effort(&entry_path, trash_backup.as_ref());
                remove_temp_backup_best_effort(trash_backup.as_ref());
                let rollback_errors =
                    restore_registry_audit_state_best_effort(&paths, &registry_backup);
                unstage_trash_paths(&self.ctx, &[&trash_rel]);
                return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
            }
        };

        if let Err(err) = stage_trash_commit_paths(&self.ctx, &[&trash_rel]) {
            restore_temp_backup_best_effort(&entry_path, trash_backup.as_ref());
            remove_temp_backup_best_effort(trash_backup.as_ref());
            let rollback_errors =
                restore_registry_audit_state_best_effort(&paths, &registry_backup);
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
                restore_temp_backup_best_effort(&entry_path, trash_backup.as_ref());
                remove_temp_backup_best_effort(trash_backup.as_ref());
                let rollback_errors =
                    restore_registry_audit_state_best_effort(&paths, &registry_backup);
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

fn rollback_trash_add(skill_path: &Path, trash_skill_path: &Path, entry_path: &Path) {
    if trash_skill_path.exists() {
        if let Some(parent) = skill_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(trash_skill_path, skill_path);
    }
    let _ = remove_path_if_exists(entry_path);
}

fn rollback_restore_from_backup(
    skill_path: &Path,
    entry_path: &Path,
    backup: Option<&serde_json::Value>,
) {
    let _ = remove_path_if_exists(skill_path);
    restore_temp_backup_best_effort(entry_path, backup);
}

fn restore_temp_backup_best_effort(path: &Path, backup: Option<&serde_json::Value>) {
    if let Some(backup) = backup {
        let _ = restore_path_from_backup(path, backup);
    }
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
