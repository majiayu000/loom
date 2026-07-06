use super::*;

pub(crate) fn maybe_skill_fault(tag: &str) -> std::result::Result<(), CommandFailure> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            format!("fault injected at {}", tag),
        ));
    }
    Ok(())
}

pub(crate) fn rollback_fault_active(tag: &str) -> bool {
    std::env::var("LOOM_ROLLBACK_FAULT_INJECT").ok().as_deref() == Some(tag)
}

pub(crate) fn push_rollback_error(errors: &mut Vec<Value>, step: &str, message: impl ToString) {
    errors.push(json!({
        "step": step,
        "message": message.to_string(),
    }));
}

pub(super) fn maybe_push_rollback_fault(errors: &mut Vec<Value>, step: &str) -> bool {
    if rollback_fault_active(step) {
        push_rollback_error(errors, step, format!("fault injected at {}", step));
        return true;
    }
    false
}

pub(super) fn rollback_capture_mutation(
    ctx: &crate::state::AppContext,
    skill_path: &Path,
    source_backup: Option<&serde_json::Value>,
    source_replaced: bool,
    previous_head: &str,
    previous_index: &gitops::IndexSnapshot,
    commit_created: bool,
) -> Vec<Value> {
    let mut errors = Vec::new();
    if commit_created {
        // Preserve worktree content while removing only the command-created commit.
        if !maybe_push_rollback_fault(&mut errors, "reset_command_created_commit") {
            match gitops::run_git_allow_failure(ctx, &["reset", "--soft", previous_head]) {
                Ok(output) if output.status.success() => {}
                Ok(output) => push_rollback_error(
                    &mut errors,
                    "reset_command_created_commit",
                    String::from_utf8_lossy(&output.stderr).trim(),
                ),
                Err(err) => push_rollback_error(&mut errors, "reset_command_created_commit", err),
            }
        }
    }

    if source_replaced {
        if let Some(backup) = source_backup {
            if !maybe_push_rollback_fault(&mut errors, "restore_source_path")
                && let Err(err) = restore_path_from_backup(skill_path, backup)
            {
                push_rollback_error(&mut errors, "restore_source_path", err);
            }
        } else {
            if !maybe_push_rollback_fault(&mut errors, "remove_source_path")
                && let Err(err) = remove_path_if_exists(skill_path)
            {
                push_rollback_error(&mut errors, "remove_source_path", err);
            }
        }
    }

    if !maybe_push_rollback_fault(&mut errors, "restore_git_index")
        && let Err(err) = gitops::restore_index(ctx, previous_index)
    {
        push_rollback_error(&mut errors, "restore_git_index", err);
    }
    errors
}

pub(super) fn ensure_capture_source_not_drifted(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
    skill_rel: &Path,
) -> std::result::Result<(), CommandFailure> {
    let skill_rel_str = skill_rel.to_string_lossy();
    let committed = git_diff_has_changes(
        ctx,
        &[&projection.last_applied_rev, "HEAD", "--", &skill_rel_str],
    )?;
    let staged = git_diff_has_changes(ctx, &["--cached", "--", &skill_rel_str])?;
    let unstaged = git_diff_has_changes(ctx, &["--", &skill_rel_str])?;

    if !(committed || staged || unstaged) {
        return Ok(());
    }

    let current_rev = gitops::head(ctx).map_err(map_git)?;
    let mut failure = CommandFailure::new(
        ErrorCode::CaptureConflict,
        format!(
            "source skill '{}' changed since projection '{}'; save or rollback source changes before capture",
            projection.skill_id, projection.instance_id
        ),
    );
    failure.details = json!({
        "skill_id": projection.skill_id,
        "instance_id": projection.instance_id,
        "source_path": skill_rel.display().to_string(),
        "last_applied_rev": projection.last_applied_rev,
        "current_rev": current_rev,
        "committed": committed,
        "staged": staged,
        "unstaged": unstaged
    });
    Err(failure)
}

pub(super) fn git_diff_has_changes(
    ctx: &crate::state::AppContext,
    args: &[&str],
) -> std::result::Result<bool, CommandFailure> {
    let mut full_args = vec!["diff", "--quiet"];
    full_args.extend(args.iter().copied());
    let output = gitops::run_git_allow_failure(ctx, &full_args).map_err(map_git)?;
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(map_git(anyhow::anyhow!(
            "git {:?} failed: {}",
            full_args,
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

pub(crate) fn rollback_registry_state(
    paths: &RegistryStatePaths,
    original_bindings: &RegistryBindingsFile,
    original_rules: &RegistryRulesFile,
    original_projections: &RegistryProjectionsFile,
) -> anyhow::Result<()> {
    if rollback_fault_active("restore_registry_state") {
        anyhow::bail!("fault injected at restore_registry_state");
    }
    paths.save_bindings_rules_projections(
        original_bindings,
        original_rules,
        original_projections,
    )?;
    Ok(())
}

#[derive(Debug, Default)]
pub(super) struct MonitorTotals {
    imported: usize,
    updated: usize,
    unchanged: usize,
    skipped: usize,
}

impl MonitorTotals {
    pub(super) fn add_cycle(&mut self, cycle: &serde_json::Value) {
        self.imported += cycle["imported_count"].as_u64().unwrap_or(0) as usize;
        self.updated += cycle["updated_count"].as_u64().unwrap_or(0) as usize;
        self.unchanged += cycle["unchanged_count"].as_u64().unwrap_or(0) as usize;
        self.skipped += cycle["skipped_count"].as_u64().unwrap_or(0) as usize;
    }

    pub(super) fn to_json(&self) -> serde_json::Value {
        json!({
            "imported": self.imported,
            "updated": self.updated,
            "unchanged": self.unchanged,
            "skipped": self.skipped,
            "changed": self.imported + self.updated,
        })
    }
}

#[derive(Debug)]
pub(super) struct MonitorUpdateRollback {
    pub(super) skill_rel: String,
    pub(super) dst: PathBuf,
    pub(super) previous: PathBuf,
}

pub(super) fn merge_monitor_meta(meta: &mut Meta, cycle_meta: Meta) {
    if cycle_meta.op_id.is_some() {
        meta.op_id = cycle_meta.op_id;
    }
    if cycle_meta.sync_state.is_some() {
        meta.sync_state = cycle_meta.sync_state;
    }
    meta.warnings.extend(cycle_meta.warnings);
}

pub(super) fn record_observed_skill_events<'a>(
    paths: &RegistryStatePaths,
    projections: &[RegistryProjectionInstance],
    changes: impl Iterator<Item = &'a serde_json::Value>,
    commit: &str,
) -> anyhow::Result<()> {
    for item in changes {
        let Some(skill_id) = item["skill"].as_str() else {
            continue;
        };
        let path = item["source"]
            .as_str()
            .or_else(|| item["path"].as_str())
            .map(str::to_string);
        for projection in projections.iter().filter(|p| p.skill_id == skill_id) {
            record_registry_observation(
                paths,
                &projection.instance_id,
                "monitor",
                path.clone(),
                None,
                Some(commit.to_string()),
            )?;
        }
    }
    Ok(())
}

pub(super) fn observed_import_targets(
    targets: &[RegistryProjectionTarget],
    target_id: Option<&str>,
) -> std::result::Result<Vec<RegistryProjectionTarget>, CommandFailure> {
    if let Some(target_id) = target_id {
        let target = targets
            .iter()
            .find(|target| target.target_id == target_id)
            .cloned()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::TargetNotFound,
                    format!("target '{}' not found", target_id),
                )
            })?;
        if target.ownership != "observed" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' has ownership '{}' and cannot be imported as observed",
                    target.target_id, target.ownership
                ),
            ));
        }
        return Ok(vec![target]);
    }

    Ok(targets
        .iter()
        .filter(|target| target.ownership == "observed")
        .cloned()
        .collect())
}

pub(super) fn observed_skill_copy_source(
    source_path: &Path,
    file_type: &fs::FileType,
    skipped: &mut Vec<serde_json::Value>,
    target: &RegistryProjectionTarget,
) -> Option<(PathBuf, &'static str, Option<String>)> {
    if file_type.is_dir() {
        return Some((source_path.to_path_buf(), "directory", None));
    }
    if !file_type.is_symlink() {
        return None;
    }

    let metadata = match fs::metadata(source_path) {
        Ok(metadata) => metadata,
        Err(err) => {
            skipped.push(json!({
                "target_id": target.target_id.clone(),
                "source": source_path.display().to_string(),
                "reason": "symlink-target-failed",
                "error": err.to_string(),
            }));
            return None;
        }
    };
    if !metadata.is_dir() {
        return None;
    }

    match fs::canonicalize(source_path) {
        Ok(resolved) => {
            let display = resolved.display().to_string();
            Some((resolved, "symlink", Some(display)))
        }
        Err(err) => {
            skipped.push(json!({
                "target_id": target.target_id.clone(),
                "source": source_path.display().to_string(),
                "reason": "symlink-resolve-failed",
                "error": err.to_string(),
            }));
            None
        }
    }
}

pub(super) fn has_skill_entrypoint(path: &Path) -> bool {
    path.join("SKILL.md").is_file() || path.join("skill.md").is_file()
}

pub(super) fn materialized_dirs_equal(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let left_files = collect_materialized_files(left)?;
    let right_files = collect_materialized_files(right)?;
    if left_files.len() != right_files.len() {
        return Ok(false);
    }

    for (rel, left_body) in left_files {
        match right_files.get(&rel) {
            Some(right_body) if right_body == &left_body => {}
            _ => return Ok(false),
        }
    }

    Ok(true)
}

pub(super) fn collect_materialized_files(
    root: &Path,
) -> anyhow::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", root.display()))?;
        let rel = entry.path().strip_prefix(root).with_context(|| {
            format!(
                "failed to derive relative path for {} under {}",
                entry.path().display(),
                root.display()
            )
        })?;
        if rel.as_os_str().is_empty() || entry.file_type().is_dir() {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Ok(BTreeMap::from([(
                rel.to_path_buf(),
                b"__loom_symlink_marker__".to_vec(),
            )]));
        }
        if entry.file_type().is_file() {
            let body = fs::read(entry.path())
                .with_context(|| format!("failed to read {}", entry.path().display()))?;
            files.insert(rel.to_path_buf(), body);
        }
    }

    Ok(files)
}

pub(super) fn reset_command_created_commits(ctx: &crate::state::AppContext, previous_head: &str) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "--soft", previous_head]);
}

pub(super) fn unstage_registry_state(ctx: &crate::state::AppContext) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/registry"]);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/v3"]);
}

pub(super) fn stage_registry_state(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
) -> std::result::Result<(), CommandFailure> {
    gitops::run_git(ctx, &["add", "-A", "--", "state/registry"]).map_err(map_git)?;
    let legacy_v3_tracked =
        gitops::run_git_allow_failure(ctx, &["ls-files", "--error-unmatch", "--", "state/v3"])
            .map_err(map_git)?
            .status
            .success();
    if paths.state_dir.join("v3").exists() || legacy_v3_tracked {
        gitops::run_git(ctx, &["add", "-A", "--", "state/v3"]).map_err(map_git)?;
    }
    Ok(())
}

pub(super) fn rollback_registry_audit_after_failure(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
    had_registry_layout: bool,
    had_legacy_layout: bool,
    legacy_layout_backup: Option<&serde_json::Value>,
) {
    let _ = restore_registry_audit_state(paths, registry_backup);
    rollback_registry_layout_after_failure(
        ctx,
        paths,
        had_registry_layout,
        had_legacy_layout,
        legacy_layout_backup,
    );
}

pub(super) fn rollback_registry_layout_after_failure(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    had_registry_layout: bool,
    had_legacy_layout: bool,
    legacy_layout_backup: Option<&serde_json::Value>,
) {
    if had_legacy_layout && !had_registry_layout {
        let legacy_dir = paths.state_dir.join("v3");
        if let Some(backup) = legacy_layout_backup {
            match remove_path_if_exists(&paths.registry_dir) {
                Ok(()) | Err(_) => {}
            }
            match restore_path_from_backup(&legacy_dir, backup) {
                Ok(()) | Err(_) => {}
            }
        } else {
            match remove_path_if_exists(&legacy_dir) {
                Ok(()) | Err(_) => {}
            }
            match fs::rename(&paths.registry_dir, legacy_dir) {
                Ok(()) | Err(_) => {}
            }
        }
    } else if !had_registry_layout && !had_legacy_layout {
        let _ = remove_path_if_exists(&paths.registry_dir);
    }
    remove_backup_path_best_effort(legacy_layout_backup);
    unstage_registry_state(ctx);
}

pub(super) fn remove_backup_path_best_effort(backup: Option<&serde_json::Value>) {
    let Some(path) = backup
        .and_then(|backup| backup.get("backup_path"))
        .and_then(serde_json::Value::as_str)
        .map(Path::new)
    else {
        return;
    };
    match remove_path_if_exists(path) {
        Ok(()) | Err(_) => {}
    }
    if let Some(parent) = path.parent() {
        match fs::remove_dir(parent) {
            Ok(()) | Err(_) => {}
        }
        if let Some(grandparent) = parent.parent() {
            match fs::remove_dir(grandparent) {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

pub(super) fn rollback_import_after_commit(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
    previous_head: &str,
    imported_rels: &[String],
) {
    reset_command_created_commits(ctx, previous_head);
    rollback_imported_skills(ctx, imported_rels);
    let _ = restore_registry_audit_state(paths, registry_backup);
    unstage_registry_state(ctx);
}

pub(super) fn rollback_monitor_after_commit(
    ctx: &crate::state::AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
    previous_head: &str,
    imported_rels: &[String],
    update_rollbacks: &[MonitorUpdateRollback],
) {
    reset_command_created_commits(ctx, previous_head);
    rollback_monitor_changes(ctx, imported_rels, update_rollbacks);
    let _ = restore_registry_audit_state(paths, registry_backup);
    unstage_registry_state(ctx);
}

pub(super) fn rollback_monitor_changes(
    ctx: &crate::state::AppContext,
    imported_rels: &[String],
    update_rollbacks: &[MonitorUpdateRollback],
) {
    for update in update_rollbacks.iter().rev() {
        let _ = remove_path_if_exists(&update.dst);
        let _ = fs::rename(&update.previous, &update.dst);
        let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", &update.skill_rel]);
    }

    rollback_imported_skills(ctx, imported_rels);
}

pub(super) fn rollback_imported_skills(ctx: &crate::state::AppContext, skill_rels: &[String]) {
    for skill_rel in skill_rels {
        let dst = ctx.root.join(skill_rel);
        rollback_added_skill(ctx, skill_rel, &dst);
    }
}
