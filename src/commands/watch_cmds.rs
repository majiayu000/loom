use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::cli::WatchArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::state::remove_path_if_exists;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::helpers::{
    RegistryAuditStateBackup, map_arg, map_git, map_io, map_lock, map_registry_state,
    record_registry_operation, restore_registry_audit_state, snapshot_registry_audit_state,
    validate_skill_name,
};
use super::{App, CommandFailure};
use walkdir::WalkDir;

#[derive(Debug, Clone, Eq, PartialEq)]
struct WatchPlan {
    skills: Vec<WatchSkillPlan>,
}

impl WatchPlan {
    fn empty() -> Self {
        Self { skills: Vec::new() }
    }

    fn is_empty(&self) -> bool {
        self.skills.iter().all(|skill| skill.paths.is_empty())
    }

    fn path_count(&self) -> usize {
        self.skills.iter().map(|skill| skill.paths.len()).sum()
    }

    fn dry_run_json(&self) -> Vec<Value> {
        self.skills
            .iter()
            .map(|skill| {
                json!({
                    "skill": skill.skill,
                    "paths": skill.paths,
                    "would_commit": !skill.paths.is_empty(),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct WatchSkillPlan {
    skill: String,
    paths: Vec<String>,
}

impl App {
    pub fn cmd_watch(
        &self,
        args: &WatchArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        if args.max_batch == 0 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--max-batch must be greater than 0",
            ));
        }
        if !args.once && args.debounce_ms == 0 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--debounce-ms must be greater than 0 for long-running watch",
            ));
        }

        if args.once {
            return self.watch_once(args, request_id);
        }

        let mut cycles = 0_u64;
        let mut saved_total = 0_usize;
        let mut skipped_total = 0_usize;
        let mut last_cycle = json!(null);
        let mut meta = Meta::default();

        loop {
            let (cycle, cycle_meta) = self.watch_once(args, request_id)?;
            cycles += 1;
            saved_total += cycle["saved_skills"].as_array().map(Vec::len).unwrap_or(0);
            skipped_total += cycle["skipped"].as_array().map(Vec::len).unwrap_or(0);
            last_cycle = cycle;
            merge_watch_meta(&mut meta, cycle_meta);

            if args.max_cycles.is_some_and(|max| cycles >= max) {
                break;
            }

            thread::sleep(Duration::from_millis(args.debounce_ms));
        }

        Ok((
            json!({
                "cycles": cycles,
                "saved_total": saved_total,
                "skipped_total": skipped_total,
                "last_cycle": last_cycle,
            }),
            meta,
        ))
    }

    fn watch_once(
        &self,
        args: &WatchArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_watch_scope(self, args)?;
        if args.dry_run {
            let plan = collect_dry_run_watch_plan(&self.ctx, args)?;
            return watch_dry_run_result(plan, args);
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_no_unresolved_conflicts(&self.ctx)?;

        let plan = collect_stable_watch_plan(&self.ctx, args)?;
        if plan.is_empty() {
            return Ok((
                json!({
                    "changed_skills": [],
                    "saved_skills": [],
                    "skipped": [],
                    "dry_run": false,
                    "noop": true,
                }),
                Meta::default(),
            ));
        }

        let path_count = plan.path_count();
        if path_count > args.max_batch {
            return Err(CommandFailure::new(
                ErrorCode::DependencyConflict,
                format!(
                    "watch batch has {} changed paths, exceeding --max-batch {}; run manual skill save",
                    path_count, args.max_batch
                ),
            ));
        }

        self.autosave_watch_plan(plan, request_id)
    }

    fn autosave_watch_plan(
        &self,
        plan: WatchPlan,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let mut saved = Vec::new();
        let mut skipped = Vec::new();
        let mut meta = Meta::default();

        for skill in plan.skills {
            let lock = match self.ctx.lock_skill(&skill.skill) {
                Ok(lock) => lock,
                Err(err) => {
                    let message = err.to_string();
                    meta.warnings.push(format!(
                        "skill '{}' lock busy; skipped autosave: {}",
                        skill.skill, message
                    ));
                    skipped.push(json!({
                        "skill": skill.skill,
                        "paths": skill.paths,
                        "reason": "lock-busy",
                    }));
                    continue;
                }
            };

            let saved_skill = self.autosave_skill(&skill, request_id)?;
            drop(lock);
            if !saved_skill["noop"].as_bool().unwrap_or(false) {
                if let Some(op_id) = saved_skill["op_id"].as_str() {
                    meta.op_id = Some(op_id.to_string());
                }
                saved.push(saved_skill);
            }
        }

        Ok((
            json!({
                "changed_skills": [],
                "saved_skills": saved,
                "skipped": skipped,
                "dry_run": false,
                "noop": saved.is_empty(),
            }),
            meta,
        ))
    }

    fn autosave_skill(
        &self,
        skill: &WatchSkillPlan,
        request_id: &str,
    ) -> std::result::Result<serde_json::Value, CommandFailure> {
        stage_watch_paths(&self.ctx, &skill.paths)?;
        let changed = has_staged_changes_for_paths(&self.ctx, &skill.paths)?;
        if !changed {
            return Ok(json!({
                "skill": skill.skill,
                "paths": skill.paths,
                "noop": true,
            }));
        }

        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let had_registry_layout = paths.registry_dir.exists();
        let had_legacy_layout = paths.legacy_state_dir_exists();
        paths.ensure_layout().map_err(map_registry_state)?;
        let registry_backup = snapshot_registry_audit_state(&paths).map_err(map_registry_state)?;

        let op_id = match record_registry_operation(
            &paths,
            "skill.autosave",
            json!({
                "skill": skill.skill,
                "paths": skill.paths,
                "request_id": request_id,
            }),
            json!({
                "noop": false,
            }),
        ) {
            Ok(op_id) => op_id,
            Err(err) => {
                unstage_watch_paths(&self.ctx, &skill.paths);
                rollback_autosave_registry_audit_after_failure(
                    &self.ctx,
                    &paths,
                    &registry_backup,
                    had_registry_layout,
                    had_legacy_layout,
                );
                return Err(map_registry_state(err));
            }
        };

        if let Err(err) = stage_autosave_registry_state(&self.ctx, &paths) {
            unstage_watch_paths(&self.ctx, &skill.paths);
            rollback_autosave_registry_audit_after_failure(
                &self.ctx,
                &paths,
                &registry_backup,
                had_registry_layout,
                had_legacy_layout,
            );
            return Err(err);
        }

        let message = format!("autosave({}): captured local edits", skill.skill);
        let commit = match commit_autosave_paths(&self.ctx, &skill.paths, &paths, &message) {
            Ok(commit) => commit,
            Err(err) => {
                unstage_watch_paths(&self.ctx, &skill.paths);
                rollback_autosave_registry_audit_after_failure(
                    &self.ctx,
                    &paths,
                    &registry_backup,
                    had_registry_layout,
                    had_legacy_layout,
                );
                return Err(map_git(err));
            }
        };

        Ok(json!({
            "skill": skill.skill,
            "paths": skill.paths,
            "commit": commit,
            "op_id": op_id,
            "noop": false,
        }))
    }
}

fn watch_dry_run_result(
    plan: WatchPlan,
    args: &WatchArgs,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    if plan.is_empty() {
        return Ok((
            json!({
                "changed_skills": [],
                "saved_skills": [],
                "skipped": [],
                "dry_run": true,
                "noop": true,
            }),
            Meta::default(),
        ));
    }

    let path_count = plan.path_count();
    if path_count > args.max_batch {
        return Err(CommandFailure::new(
            ErrorCode::DependencyConflict,
            format!(
                "watch batch has {} changed paths, exceeding --max-batch {}; run manual skill save",
                path_count, args.max_batch
            ),
        ));
    }

    Ok((
        json!({
            "changed_skills": plan.dry_run_json(),
            "saved_skills": [],
            "skipped": [],
            "dry_run": true,
            "noop": false,
        }),
        Meta::default(),
    ))
}

fn validate_watch_scope(app: &App, args: &WatchArgs) -> std::result::Result<(), CommandFailure> {
    if let Some(skill) = &args.skill {
        validate_skill_name(skill).map_err(map_arg)?;
        if !app.ctx.skill_path(skill).exists() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", skill),
            ));
        }
    }
    Ok(())
}

fn collect_dry_run_watch_plan(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchPlan, CommandFailure> {
    if gitops::repo_is_initialized(ctx).map_err(map_git)? {
        return collect_watch_plan(ctx, args);
    }

    collect_uninitialized_watch_plan(ctx, args)
}

fn collect_uninitialized_watch_plan(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchPlan, CommandFailure> {
    let mut grouped: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (skill, root) in uninitialized_watch_roots(ctx, args)? {
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                if entry.depth() == 0 {
                    return true;
                }
                let Ok(rel) = entry.path().strip_prefix(&root) else {
                    return true;
                };
                let rel = rel
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                !is_ignored_skill_rel(&rel)
            })
        {
            let entry = entry.map_err(map_io)?;
            if entry.depth() == 0 {
                continue;
            }
            let file_type = entry.file_type();
            if !(file_type.is_file() || file_type.is_symlink()) {
                continue;
            }
            let rel = entry.path().strip_prefix(&root).map_err(map_io)?;
            let rel = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if is_ignored_skill_rel(&rel) {
                continue;
            }
            grouped
                .entry(skill.clone())
                .or_default()
                .insert(format!("skills/{}/{}", skill, rel));
        }
    }

    if grouped.is_empty() {
        return Ok(WatchPlan::empty());
    }

    Ok(WatchPlan {
        skills: grouped
            .into_iter()
            .map(|(skill, paths)| WatchSkillPlan {
                skill,
                paths: paths.into_iter().collect(),
            })
            .collect(),
    })
}

fn uninitialized_watch_roots(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<Vec<(String, PathBuf)>, CommandFailure> {
    if let Some(skill) = &args.skill {
        return Ok(vec![(skill.clone(), ctx.skill_path(skill))]);
    }

    let skills_dir = ctx.root.join("skills");
    if !skills_dir.exists() {
        return Ok(Vec::new());
    }

    let mut roots = Vec::new();
    for entry in fs::read_dir(&skills_dir).map_err(map_io)? {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().map_err(map_io)?.is_dir() {
            continue;
        }
        let Some(skill) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if validate_skill_name(&skill).is_err() {
            continue;
        }
        roots.push((skill, entry.path()));
    }
    roots.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(roots)
}

fn collect_stable_watch_plan(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchPlan, CommandFailure> {
    let first = collect_watch_plan(ctx, args)?;
    if first.is_empty() || args.debounce_ms == 0 {
        return Ok(first);
    }

    thread::sleep(Duration::from_millis(args.debounce_ms));
    let second = collect_watch_plan(ctx, args)?;
    if first == second {
        return Ok(second);
    }

    thread::sleep(Duration::from_millis(args.debounce_ms));
    let third = collect_watch_plan(ctx, args)?;
    if second == third {
        return Ok(third);
    }

    Err(CommandFailure::new(
        ErrorCode::CaptureConflict,
        "skill files changed during autosave debounce; retry after edits settle",
    ))
}

fn collect_watch_plan(
    ctx: &AppContext,
    args: &WatchArgs,
) -> std::result::Result<WatchPlan, CommandFailure> {
    let scopes = watch_scopes(args);
    let mut git_args = vec![
        "status".to_string(),
        "--porcelain=v1".to_string(),
        "-z".to_string(),
        "-uall".to_string(),
        "--".to_string(),
    ];
    git_args.extend(scopes);
    let git_refs = git_args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = gitops::run_git_allow_failure(ctx, &git_refs).map_err(map_git)?;
    if !output.status.success() {
        return Err(CommandFailure::new(
            ErrorCode::GitError,
            format!(
                "git status failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }

    let mut grouped: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let records = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;
    while index < records.len() {
        let record = String::from_utf8_lossy(records[index]);
        if record.len() < 4 {
            index += 1;
            continue;
        }

        let status = &record[..2];
        let path = &record[3..];
        if let Some((skill, rel)) = skill_path_parts(path)
            && validate_skill_name(&skill).is_ok()
            && !is_ignored_skill_rel(&rel)
        {
            grouped.entry(skill).or_default().insert(path.to_string());
        }

        index += 1;
        if status.starts_with('R') || status.starts_with('C') {
            index += 1;
        }
    }

    if grouped.is_empty() {
        return Ok(WatchPlan::empty());
    }

    Ok(WatchPlan {
        skills: grouped
            .into_iter()
            .map(|(skill, paths)| WatchSkillPlan {
                skill,
                paths: paths.into_iter().collect(),
            })
            .collect(),
    })
}

fn watch_scopes(args: &WatchArgs) -> Vec<String> {
    match &args.skill {
        Some(skill) => vec![format!("skills/{}", skill)],
        None => vec!["skills".to_string()],
    }
}

fn skill_path_parts(path: &str) -> Option<(String, String)> {
    let path = path.strip_prefix("./").unwrap_or(path);
    let mut parts = path.split('/');
    if parts.next()? != "skills" {
        return None;
    }
    let skill = parts.next()?.to_string();
    if skill.is_empty() {
        return None;
    }
    let rel = parts.collect::<Vec<_>>().join("/");
    if rel.is_empty() {
        return None;
    }
    Some((skill, rel))
}

fn is_ignored_skill_rel(rel: &str) -> bool {
    let mut file_name = "";
    for part in rel.split('/') {
        if matches!(part, ".git" | "state" | "trash" | "backups") {
            return true;
        }
        file_name = part;
    }

    file_name == ".DS_Store"
        || file_name.ends_with(".swp")
        || file_name.ends_with(".tmp")
        || file_name.ends_with('~')
}

fn ensure_no_unresolved_conflicts(ctx: &AppContext) -> std::result::Result<(), CommandFailure> {
    let output = gitops::run_git_allow_failure(ctx, &["ls-files", "-u"]).map_err(map_git)?;
    if !output.status.success() {
        return Err(CommandFailure::new(
            ErrorCode::GitError,
            format!(
                "failed to inspect unresolved conflicts: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    if !output.stdout.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::GitError,
            "repository has unresolved conflicts; resolve them before autosave",
        ));
    }
    Ok(())
}

fn stage_watch_paths(
    ctx: &AppContext,
    paths: &[String],
) -> std::result::Result<(), CommandFailure> {
    run_git_with_paths(ctx, &["add", "-A", "--"], paths).map_err(map_git)?;
    Ok(())
}

fn has_staged_changes_for_paths(
    ctx: &AppContext,
    paths: &[String],
) -> std::result::Result<bool, CommandFailure> {
    let mut args = vec![
        "diff".to_string(),
        "--cached".to_string(),
        "--quiet".to_string(),
        "--".to_string(),
    ];
    args.extend(paths.iter().cloned());
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = gitops::run_git_allow_failure(ctx, &refs).map_err(map_git)?;
    if output.status.success() {
        return Ok(false);
    }
    if output.status.code() == Some(1) {
        return Ok(true);
    }

    Err(CommandFailure::new(
        ErrorCode::GitError,
        format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
}

fn unstage_watch_paths(ctx: &AppContext, paths: &[String]) {
    let _ = run_git_with_paths(ctx, &["reset", "HEAD", "--"], paths);
}

fn run_git_with_paths(
    ctx: &AppContext,
    prefix: &[&str],
    paths: &[String],
) -> anyhow::Result<String> {
    let mut args = prefix
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    args.extend(paths.iter().cloned());
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    gitops::run_git(ctx, &refs)
}

fn stage_autosave_registry_state(
    ctx: &AppContext,
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

fn commit_autosave_paths(
    ctx: &AppContext,
    skill_paths: &[String],
    paths: &RegistryStatePaths,
    message: &str,
) -> anyhow::Result<String> {
    let mut commit_paths = skill_paths.to_vec();
    commit_paths.push("state/registry".to_string());
    let legacy_v3_tracked =
        gitops::run_git_allow_failure(ctx, &["ls-files", "--error-unmatch", "--", "state/v3"])?
            .status
            .success();
    if paths.state_dir.join("v3").exists() || legacy_v3_tracked {
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

fn rollback_autosave_registry_audit_after_failure(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    registry_backup: &RegistryAuditStateBackup,
    had_registry_layout: bool,
    had_legacy_layout: bool,
) {
    let _ = restore_registry_audit_state(paths, registry_backup);
    if !had_registry_layout && !had_legacy_layout {
        let _ = remove_path_if_exists(&paths.registry_dir);
    }
    unstage_registry_state(ctx);
}

fn unstage_registry_state(ctx: &AppContext) {
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/registry"]);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", "state/v3"]);
}

fn merge_watch_meta(target: &mut Meta, source: Meta) {
    target.warnings.extend(source.warnings);
    if source.sync_state.is_some() {
        target.sync_state = source.sync_state;
    }
    if source.op_id.is_some() {
        target.op_id = source.op_id;
    }
}
