use super::shared::*;
use super::*;

impl App {
    pub fn cmd_import_observed(
        &self,
        args: &ImportObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;

        let targets = observed_import_targets(&snapshot.targets.targets, args.target.as_deref())?;
        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-import-observed-{}", Uuid::new_v4()));
        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        let mut imported_rels = Vec::new();

        for target in targets {
            let target_path = PathBuf::from(&target.path);
            if !target_path.exists() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-missing",
                }));
                continue;
            }
            if !target_path.is_dir() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-not-directory",
                }));
                continue;
            }

            let mut entries = match fs::read_dir(&target_path) {
                Ok(entries) => entries
                    .filter_map(|entry| match entry {
                        Ok(entry) => Some(entry),
                        Err(err) => {
                            skipped.push(json!({
                                "target_id": target.target_id,
                                "path": target.path,
                                "reason": "entry-read-failed",
                                "error": err.to_string(),
                            }));
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                Err(err) => {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "path": target.path,
                        "reason": "target-read-failed",
                        "error": err.to_string(),
                    }));
                    continue;
                }
            };
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let source_path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(err) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "reason": "file-type-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                };
                let (copy_source, source_kind, resolved_source) = match observed_skill_copy_source(
                    &source_path,
                    &file_type,
                    &mut skipped,
                    &target,
                ) {
                    Some(source) => source,
                    None => continue,
                };
                if !has_skill_entrypoint(&copy_source) {
                    push_missing_skill_entrypoint_skip(
                        &mut skipped,
                        &target.target_id,
                        &source_path,
                        &copy_source,
                        source_kind,
                        resolved_source.as_deref(),
                    );
                    continue;
                }

                let skill_id = match entry.file_name().into_string() {
                    Ok(name) => name,
                    Err(name) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "name": name.to_string_lossy(),
                            "reason": "non-utf8-name",
                        }));
                        continue;
                    }
                };

                if let Err(err) = validate_skill_name(&skill_id) {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "invalid-skill-name",
                        "error": err.to_string(),
                    }));
                    continue;
                }

                let dst = self.ctx.skill_path(&skill_id);
                if dst.exists() {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "already-exists",
                    }));
                    continue;
                }

                let staging_skill = staging_root.join(&skill_id);
                let _ = remove_path_if_exists(&staging_skill);
                match copy_dir_recursive_without_symlinks(&copy_source, &staging_skill) {
                    Ok(()) => {}
                    Err(err) => {
                        let _ = remove_path_if_exists(&staging_skill);
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "skill": skill_id,
                            "source": source_path.display().to_string(),
                            "reason": "copy-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                }

                if let Err(err) = fs::rename(&staging_skill, &dst) {
                    cleanup_staging();
                    let rollback_errors = rollback_imported_skills(&self.ctx, &imported_rels);
                    return Err(map_io(err).with_rollback_errors(rollback_errors));
                }

                let skill_rel = format!("skills/{}", skill_id);
                if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
                    cleanup_staging();
                    let mut rollback_errors = rollback_imported_skills(&self.ctx, &imported_rels);
                    rollback_errors.extend(rollback_added_skill(&self.ctx, &skill_rel, &dst));
                    return Err(map_git(err).with_rollback_errors(rollback_errors));
                }
                imported_rels.push(skill_rel);
                let mut imported_item = json!({
                    "target_id": target.target_id,
                    "skill": skill_id,
                    "source": source_path.display().to_string(),
                    "source_kind": source_kind,
                    "path": dst.display().to_string(),
                });
                if let Some(resolved_source) = resolved_source {
                    imported_item["resolved_source"] = json!(resolved_source);
                }
                imported.push(imported_item);
            }
        }

        cleanup_staging();

        let mut meta = Meta::default();
        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let mut changed = false;
        for skill_rel in &imported_rels {
            match gitops::has_staged_changes_for_path(&self.ctx, Path::new(skill_rel)) {
                Ok(true) => {
                    changed = true;
                    break;
                }
                Ok(false) => {}
                Err(err) => {
                    let rollback_errors = rollback_imported_skills(&self.ctx, &imported_rels);
                    return Err(map_git(err).with_rollback_errors(rollback_errors));
                }
            }
        }

        let commit = if changed {
            let message = match imported.len() {
                1 => format!(
                    "import-observed({}): from observed target",
                    imported[0]["skill"].as_str().unwrap_or("skill")
                ),
                count => format!("import-observed: {} skills", count),
            };
            let commit = match gitops::commit(&self.ctx, &message) {
                Ok(commit) => commit,
                Err(err) => {
                    let rollback_errors = rollback_imported_skills(&self.ctx, &imported_rels);
                    return Err(map_git(err).with_rollback_errors(rollback_errors));
                }
            };
            let post_commit = (|| -> std::result::Result<Meta, CommandFailure> {
                let op_id = record_registry_operation(
                    &paths,
                    "skill.import_observed",
                    json!({
                        "target": args.target,
                        "request_id": request_id
                    }),
                    json!({
                        "commit": commit,
                        "imported": imported,
                        "skipped": skipped
                    }),
                )
                .map_err(map_registry_state)?;
                let state_commit =
                    commit_registry_state(&self.ctx, "import-observed: record registry state")?;
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                maybe_autosync_or_queue(
                    &self.ctx,
                    "import-observed",
                    request_id,
                    json!({"commit": commit, "state_commit": state_commit, "count": imported.len()}),
                    &mut meta,
                )?;
                Ok(meta)
            })();
            let post_meta = match post_commit {
                Ok(result) => result,
                Err(err) => {
                    let rollback_errors = rollback_import_after_commit(
                        &self.ctx,
                        &paths,
                        &registry_backup,
                        &previous_head,
                        &imported_rels,
                    );
                    return Err(err.with_rollback_errors(rollback_errors));
                }
            };
            meta = post_meta;
            Some(commit)
        } else {
            None
        };

        Ok((
            json!({
                "count": imported.len(),
                "imported": imported,
                "skipped": skipped,
                "commit": commit,
                "noop": !changed,
            }),
            meta,
        ))
    }

    pub fn cmd_monitor_observed(
        &self,
        args: &MonitorObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        if !args.once && args.interval_seconds == 0 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--interval-seconds must be greater than 0 for long-running monitoring",
            ));
        }

        let mut cycles = 0_u64;
        let mut totals = MonitorTotals::default();
        let mut last_cycle = json!(null);
        let mut meta = Meta::default();

        loop {
            let (cycle, cycle_meta) = self.monitor_observed_once(args, request_id)?;
            cycles += 1;
            totals.add_cycle(&cycle);
            last_cycle = cycle;
            merge_monitor_meta(&mut meta, cycle_meta);

            if args.once || args.max_cycles.is_some_and(|max| cycles >= max) {
                break;
            }

            thread::sleep(Duration::from_secs(args.interval_seconds));
        }

        Ok((
            json!({
                "cycles": cycles,
                "totals": totals.to_json(),
                "last_cycle": last_cycle,
            }),
            meta,
        ))
    }

    fn monitor_observed_once(
        &self,
        args: &MonitorObservedArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_registry_state)?;
        let targets = observed_import_targets(&snapshot.targets.targets, args.target.as_deref())?;
        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-monitor-observed-{}", Uuid::new_v4()));
        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        let mut imported = Vec::new();
        let mut updated = Vec::new();
        let mut skipped = Vec::new();
        let mut unchanged_count = 0_usize;
        let mut changed_rels = Vec::new();
        let mut imported_rels = Vec::new();
        let mut update_rollbacks = Vec::new();
        let mut seen_skill_ids = BTreeSet::new();

        for target in targets {
            let target_path = PathBuf::from(&target.path);
            if !target_path.exists() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-missing",
                }));
                continue;
            }
            if !target_path.is_dir() {
                skipped.push(json!({
                    "target_id": target.target_id,
                    "path": target.path,
                    "reason": "target-not-directory",
                }));
                continue;
            }

            let mut entries = match fs::read_dir(&target_path) {
                Ok(entries) => entries
                    .filter_map(|entry| match entry {
                        Ok(entry) => Some(entry),
                        Err(err) => {
                            skipped.push(json!({
                                "target_id": target.target_id,
                                "path": target.path,
                                "reason": "entry-read-failed",
                                "error": err.to_string(),
                            }));
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                Err(err) => {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "path": target.path,
                        "reason": "target-read-failed",
                        "error": err.to_string(),
                    }));
                    continue;
                }
            };
            entries.sort_by_key(|entry| entry.file_name());

            for entry in entries {
                let source_path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(err) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "reason": "file-type-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                };
                let (copy_source, source_kind, resolved_source) = match observed_skill_copy_source(
                    &source_path,
                    &file_type,
                    &mut skipped,
                    &target,
                ) {
                    Some(source) => source,
                    None => continue,
                };
                if !has_skill_entrypoint(&copy_source) {
                    push_missing_skill_entrypoint_skip(
                        &mut skipped,
                        &target.target_id,
                        &source_path,
                        &copy_source,
                        source_kind,
                        resolved_source.as_deref(),
                    );
                    continue;
                }

                let skill_id = match entry.file_name().into_string() {
                    Ok(name) => name,
                    Err(name) => {
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "source": source_path.display().to_string(),
                            "name": name.to_string_lossy(),
                            "reason": "non-utf8-name",
                        }));
                        continue;
                    }
                };

                if let Err(err) = validate_skill_name(&skill_id) {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "invalid-skill-name",
                        "error": err.to_string(),
                    }));
                    continue;
                }

                if !seen_skill_ids.insert(skill_id.clone()) {
                    skipped.push(json!({
                        "target_id": target.target_id,
                        "skill": skill_id,
                        "source": source_path.display().to_string(),
                        "reason": "duplicate-observed-skill",
                    }));
                    continue;
                }

                let staging_skill = staging_root.join("next").join(&skill_id);
                let _ = remove_path_if_exists(&staging_skill);
                match copy_dir_recursive_without_symlinks(&copy_source, &staging_skill) {
                    Ok(()) => {}
                    Err(err) => {
                        let _ = remove_path_if_exists(&staging_skill);
                        skipped.push(json!({
                            "target_id": target.target_id,
                            "skill": skill_id,
                            "source": source_path.display().to_string(),
                            "reason": "copy-failed",
                            "error": err.to_string(),
                        }));
                        continue;
                    }
                }

                let dst = self.ctx.skill_path(&skill_id);
                let skill_rel = format!("skills/{}", skill_id);
                let mut item = json!({
                    "target_id": target.target_id,
                    "skill": skill_id,
                    "source": source_path.display().to_string(),
                    "source_kind": source_kind,
                    "path": dst.display().to_string(),
                });
                if let Some(resolved_source) = resolved_source {
                    item["resolved_source"] = json!(resolved_source);
                }

                if dst.exists() {
                    match materialized_dirs_equal(&dst, &staging_skill) {
                        Ok(true) => {
                            unchanged_count += 1;
                            let _ = remove_path_if_exists(&staging_skill);
                            continue;
                        }
                        Ok(false) => {}
                        Err(err) => {
                            let _ = remove_path_if_exists(&staging_skill);
                            skipped.push(json!({
                                "target_id": target.target_id,
                                "skill": item["skill"].clone(),
                                "source": source_path.display().to_string(),
                                "reason": "compare-failed",
                                "error": err.to_string(),
                            }));
                            continue;
                        }
                    }

                    let previous = staging_root.join("previous").join(
                        item["skill"]
                            .as_str()
                            .expect("monitor item skill is string"),
                    );
                    if let Some(parent) = previous.parent() {
                        fs::create_dir_all(parent).map_err(map_io)?;
                    }
                    if let Err(err) = fs::rename(&dst, &previous) {
                        let rollback_errors =
                            rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_io(err).with_rollback_errors(rollback_errors));
                    }
                    if let Err(err) = fs::rename(&staging_skill, &dst) {
                        let mut rollback_errors = Vec::new();
                        if let Err(restore_err) = fs::rename(&previous, &dst) {
                            push_rollback_error(
                                &mut rollback_errors,
                                "restore_current_monitor_update",
                                restore_err,
                            );
                        }
                        rollback_errors.extend(rollback_monitor_changes(
                            &self.ctx,
                            &imported_rels,
                            &update_rollbacks,
                        ));
                        cleanup_staging();
                        return Err(map_io(err).with_rollback_errors(rollback_errors));
                    }
                    if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
                        let mut rollback_errors = Vec::new();
                        if let Err(remove_err) = remove_path_if_exists(&dst) {
                            push_rollback_error(
                                &mut rollback_errors,
                                "remove_current_monitor_update",
                                remove_err,
                            );
                        }
                        if let Err(restore_err) = fs::rename(&previous, &dst) {
                            push_rollback_error(
                                &mut rollback_errors,
                                "restore_current_monitor_update",
                                restore_err,
                            );
                        }
                        rollback_errors.extend(rollback_monitor_changes(
                            &self.ctx,
                            &imported_rels,
                            &update_rollbacks,
                        ));
                        cleanup_staging();
                        return Err(map_git(err).with_rollback_errors(rollback_errors));
                    }
                    update_rollbacks.push(MonitorUpdateRollback {
                        skill_rel: skill_rel.clone(),
                        dst,
                        previous,
                    });
                    changed_rels.push(skill_rel);
                    updated.push(item);
                } else {
                    if let Err(err) = fs::rename(&staging_skill, &dst) {
                        let rollback_errors =
                            rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                        cleanup_staging();
                        return Err(map_io(err).with_rollback_errors(rollback_errors));
                    }
                    if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
                        let mut rollback_errors = rollback_added_skill(&self.ctx, &skill_rel, &dst);
                        rollback_errors.extend(rollback_monitor_changes(
                            &self.ctx,
                            &imported_rels,
                            &update_rollbacks,
                        ));
                        cleanup_staging();
                        return Err(map_git(err).with_rollback_errors(rollback_errors));
                    }
                    imported_rels.push(skill_rel.clone());
                    changed_rels.push(skill_rel);
                    imported.push(item);
                }
            }
        }

        let mut has_changes = false;
        for skill_rel in &changed_rels {
            match gitops::has_staged_changes_for_path(&self.ctx, Path::new(skill_rel)) {
                Ok(true) => {
                    has_changes = true;
                    break;
                }
                Ok(false) => {}
                Err(err) => {
                    let rollback_errors =
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                    cleanup_staging();
                    return Err(map_git(err).with_rollback_errors(rollback_errors));
                }
            }
        }

        let mut meta = Meta::default();
        let previous_head = gitops::head(&self.ctx).map_err(map_git)?;
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;
        let commit = if has_changes {
            let change_count = imported.len() + updated.len();
            let message = if change_count == 1 {
                let skill = imported
                    .first()
                    .or_else(|| updated.first())
                    .and_then(|item| item["skill"].as_str())
                    .unwrap_or("skill");
                format!("monitor-observed({}): sync observed skill", skill)
            } else {
                format!("monitor-observed: {} skills", change_count)
            };
            let commit = match gitops::commit(&self.ctx, &message) {
                Ok(commit) => commit,
                Err(err) => {
                    let rollback_errors =
                        rollback_monitor_changes(&self.ctx, &imported_rels, &update_rollbacks);
                    cleanup_staging();
                    return Err(map_git(err).with_rollback_errors(rollback_errors));
                }
            };
            let post_commit = (|| -> std::result::Result<Meta, CommandFailure> {
                let op_id = record_registry_operation(
                    &paths,
                    "skill.monitor_observed",
                    json!({
                        "target": args.target,
                        "request_id": request_id
                    }),
                    json!({
                        "commit": commit,
                        "imported": imported,
                        "updated": updated,
                        "skipped": skipped,
                        "unchanged_count": unchanged_count,
                    }),
                )
                .map_err(map_registry_state)?;
                record_observed_skill_events(
                    &paths,
                    &snapshot.projections.projections,
                    imported.iter().chain(updated.iter()),
                    &commit,
                )
                .map_err(map_registry_state)?;
                maybe_skill_fault("skill_monitor_after_observation")?;
                let state_commit =
                    commit_registry_state(&self.ctx, "monitor-observed: record registry state")?;
                let mut meta = Meta {
                    op_id: Some(op_id),
                    ..Meta::default()
                };
                maybe_autosync_or_queue(
                    &self.ctx,
                    "monitor-observed",
                    request_id,
                    json!({
                        "commit": commit,
                        "state_commit": state_commit,
                        "imported": imported.len(),
                        "updated": updated.len(),
                    }),
                    &mut meta,
                )?;
                Ok(meta)
            })();
            let post_meta = match post_commit {
                Ok(result) => result,
                Err(err) => {
                    let rollback_errors = rollback_monitor_after_commit(
                        &self.ctx,
                        &paths,
                        &registry_backup,
                        &previous_head,
                        &imported_rels,
                        &update_rollbacks,
                    );
                    cleanup_staging();
                    return Err(err.with_rollback_errors(rollback_errors));
                }
            };
            meta = post_meta;
            Some(commit)
        } else {
            None
        };

        cleanup_staging();
        let change_count = imported.len() + updated.len();
        Ok((
            json!({
                "count": change_count,
                "imported_count": imported.len(),
                "updated_count": updated.len(),
                "unchanged_count": unchanged_count,
                "skipped_count": skipped.len(),
                "imported": imported,
                "updated": updated,
                "skipped": skipped,
                "commit": commit,
                "noop": !has_changes,
            }),
            meta,
        ))
    }
}

fn push_missing_skill_entrypoint_skip(
    skipped: &mut Vec<serde_json::Value>,
    target_id: &str,
    source_path: &Path,
    copy_source: &Path,
    source_kind: &str,
    resolved_source: Option<&str>,
) {
    let mut item = json!({
        "target_id": target_id,
        "source": source_path.display().to_string(),
        "copy_source": copy_source.display().to_string(),
        "source_kind": source_kind,
        "reason": "missing-skill-entrypoint",
    });
    if let Some(name) = source_path.file_name() {
        item["name"] = json!(name.to_string_lossy());
    }
    if let Some(resolved_source) = resolved_source {
        item["resolved_source"] = json!(resolved_source);
    }
    skipped.push(item);
}
