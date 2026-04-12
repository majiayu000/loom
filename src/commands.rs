use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::cli::{
    AddArgs, AgentKind, BindingAddArgs, CaptureArgs, Cli, Command, DiffArgs,
    HistoryRepairStrategyArg, OpsCommand, OpsHistoryCommand, ProjectArgs, ProjectionMethod,
    ReleaseArgs, RemoteCommand, RollbackArgs, SaveArgs, SkillCommand, SkillOnlyArgs, SyncCommand,
    TargetAddArgs, TargetCommand, TargetOwnership, WorkspaceBindingCommand, WorkspaceCommand,
    WorkspaceMatcherKind,
};
use crate::envelope::{Envelope, Meta};
use crate::gitops;
use crate::state::{
    AppContext, PendingOpsReport, remove_path_if_exists, resolve_agent_skill_dirs,
    resolve_agent_skill_source_dirs,
};
use crate::state_model::{
    V3BindingRule, V3BindingsFile, V3OperationRecord, V3ProjectionInstance, V3ProjectionTarget,
    V3ProjectionsFile, V3RulesFile, V3Snapshot, V3StatePaths, V3TargetCapabilities, V3TargetsFile,
    V3WorkspaceBinding, V3WorkspaceMatcher,
};
use crate::types::{ErrorCode, SyncState};

#[derive(Debug)]
pub struct CommandFailure {
    pub code: ErrorCode,
    pub message: String,
    pub details: serde_json::Value,
}

impl CommandFailure {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: json!({}),
        }
    }
}

pub struct App {
    pub ctx: AppContext,
}

impl App {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let ctx = AppContext::new(root)?;
        Ok(Self { ctx })
    }

    fn ensure_write_layout(&self) -> std::result::Result<(), CommandFailure> {
        self.ctx.ensure_state_layout().map_err(map_io)?;
        Ok(())
    }

    fn ensure_write_repo_ready(&self) -> std::result::Result<(), CommandFailure> {
        self.ensure_write_layout()?;
        gitops::ensure_repo_initialized(&self.ctx).map_err(map_git)?;
        self.ctx.ensure_gitignore_entries().map_err(map_io)?;
        ensure_initial_commit(&self.ctx).map_err(map_git)?;
        Ok(())
    }

    pub fn execute(&self, cli: Cli) -> Result<(Envelope, i32)> {
        let request_id = cli.request_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let result = match &cli.command {
            Command::Workspace { command } => match command {
                WorkspaceCommand::Status => self.cmd_status(),
                WorkspaceCommand::Doctor => self.cmd_doctor(),
                WorkspaceCommand::Binding { command } => {
                    self.cmd_workspace_binding(command, &request_id)
                }
                WorkspaceCommand::Remote { command } => self.cmd_remote(command),
            },
            Command::Target { command } => self.cmd_target(command, &request_id),
            Command::Skill { command } => match command {
                SkillCommand::Add(args) => self.cmd_add(args, &request_id),
                SkillCommand::Project(args) => self.cmd_project(args, &request_id),
                SkillCommand::Capture(args) => self.cmd_capture(args, &request_id),
                SkillCommand::Save(args) => self.cmd_save(args, &request_id),
                SkillCommand::Snapshot(args) => self.cmd_snapshot(args, &request_id),
                SkillCommand::Release(args) => self.cmd_release(args, &request_id),
                SkillCommand::Rollback(args) => self.cmd_rollback(args, &request_id),
                SkillCommand::Diff(args) => self.cmd_diff(args),
            },
            Command::Sync { command } => self.cmd_sync(command),
            Command::Ops { command } => self.cmd_ops(command),
            Command::Panel(_) => Ok((json!({"message": "panel handled in main"}), Meta::default())),
        };

        match result {
            Ok((data, meta)) => {
                let env = Envelope::ok(command_name(&cli.command), request_id, data, meta);
                Ok((env, 0))
            }
            Err(f) => {
                let env = Envelope::err(
                    command_name(&cli.command),
                    request_id,
                    f.code,
                    f.message,
                    f.details,
                );
                Ok((env, f.code.exit_code()))
            }
        }
    }

    pub fn cmd_add(
        &self,
        args: &AddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.name).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let dst = self.ctx.skill_path(&args.name);
        if dst.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("skill '{}' already exists", args.name),
            ));
        }

        let staging_root = self
            .ctx
            .state_dir
            .join(format!("tmp-add-{}", Uuid::new_v4()));
        let staging_skill = staging_root.join(&args.name);
        let clone_tmp = staging_root.join("clone");

        let cleanup_staging = || {
            let _ = remove_path_if_exists(&staging_root);
        };

        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        if Path::new(&args.source).exists() {
            if let Err(err) =
                copy_dir_recursive_without_symlinks(Path::new(&args.source), &staging_skill)
            {
                cleanup_staging();
                return Err(map_io(err));
            }
        } else {
            let source = args.source.as_str();
            let clone = gitops::run_git_allow_failure(
                &self.ctx,
                &[
                    "clone",
                    "--depth",
                    "1",
                    source,
                    clone_tmp.to_string_lossy().as_ref(),
                ],
            )
            .map_err(map_git)?;
            if !clone.status.success() {
                let stderr = String::from_utf8_lossy(&clone.stderr).to_string();
                cleanup_staging();
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("failed to clone source: {}", stderr.trim()),
                ));
            }
            if let Err(err) = copy_dir_recursive_without_symlinks(&clone_tmp, &staging_skill) {
                cleanup_staging();
                return Err(map_io(err));
            }
        }

        if let Err(err) = fs::rename(&staging_skill, &dst) {
            cleanup_staging();
            return Err(map_io(err));
        }
        cleanup_staging();

        let mut meta = Meta::default();
        let skill_rel = format!("skills/{}", args.name);
        if let Err(err) = gitops::stage_path(&self.ctx, Path::new(&skill_rel)) {
            rollback_added_skill(&self.ctx, &skill_rel, &dst);
            return Err(map_git(err));
        }
        let staged = match gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel)) {
            Ok(staged) => staged,
            Err(err) => {
                rollback_added_skill(&self.ctx, &skill_rel, &dst);
                return Err(map_git(err));
            }
        };
        if staged {
            let message = format!("add({}): import {}", args.name, args.source);
            let commit = match gitops::commit(&self.ctx, &message) {
                Ok(commit) => commit,
                Err(err) => {
                    rollback_added_skill(&self.ctx, &skill_rel, &dst);
                    return Err(map_git(err));
                }
            };
            if let Err(err) = maybe_autosync_or_queue(
                &self.ctx,
                "add",
                request_id,
                json!({"skill": args.name, "commit": commit}),
                &mut meta,
            ) {
                rollback_added_skill(&self.ctx, &skill_rel, &dst);
                return Err(err);
            }
        }

        Ok((json!({"skill": args.name, "path": dst}), meta))
    }

    pub fn cmd_project(
        &self,
        args: &ProjectArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;

        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let binding = snapshot.binding(&args.binding).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{}' not found", args.binding),
            )
        })?;

        let target_id = args
            .target
            .clone()
            .unwrap_or_else(|| binding.default_target_id.clone());
        let target = snapshot.target(&target_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", target_id),
            )
        })?;

        if target.ownership != "managed" {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' has ownership '{}' and cannot be projected into",
                    target.target_id, target.ownership
                ),
            ));
        }

        validate_projection_method(&target, args.method)?;

        let skill_src = self.ctx.skill_path(&args.skill);
        let target_base = PathBuf::from(&target.path);
        fs::create_dir_all(&target_base).map_err(map_io)?;
        let materialized_path = target_base.join(&args.skill);
        let replaced_projection_backup =
            backup_path_if_exists(&self.ctx, &materialized_path, "project.replace_projection")
                .map_err(map_io)?;
        remove_path_if_exists(&materialized_path).map_err(map_io)?;
        project_skill_to_target(&skill_src, &materialized_path, args.method)
            .map_err(map_project_io(args.method))?;

        let mut rules = snapshot.rules;
        upsert_rule(
            &mut rules,
            V3BindingRule {
                binding_id: binding.binding_id.clone(),
                skill_id: args.skill.clone(),
                target_id: target.target_id.clone(),
                method: projection_method_as_str(args.method).to_string(),
                watch_policy: "observe_only".to_string(),
                created_at: Some(Utc::now()),
            },
        );
        paths.save_rules(&rules).map_err(map_v3_state)?;

        let mut projections = snapshot.projections;
        let instance_id =
            projection_instance_id(&args.skill, &binding.binding_id, &target.target_id);
        let projection = V3ProjectionInstance {
            instance_id: instance_id.clone(),
            skill_id: args.skill.clone(),
            binding_id: binding.binding_id.clone(),
            target_id: target.target_id.clone(),
            materialized_path: materialized_path.display().to_string(),
            method: projection_method_as_str(args.method).to_string(),
            last_applied_rev: gitops::head(&self.ctx).map_err(map_git)?,
            health: "healthy".to_string(),
            observed_drift: Some(false),
            updated_at: Some(Utc::now()),
        };
        upsert_projection(&mut projections, projection.clone());
        paths.save_projections(&projections).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "skill.project",
            json!({
                "skill_id": args.skill,
                "binding_id": binding.binding_id,
                "target_id": target.target_id,
                "method": projection_method_as_str(args.method),
                "request_id": request_id
            }),
            json!({
                "instance_id": instance_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({"projection": projection, "backup": replaced_projection_backup, "noop": false}),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_capture(
        &self,
        args: &CaptureArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let projection = resolve_capture_projection(&snapshot, args)?;
        ensure_skill_exists(&self.ctx, &projection.skill_id)?;

        let skill_rel = format!("skills/{}", projection.skill_id);
        let skill_path = self.ctx.root.join(&skill_rel);
        let live_path = PathBuf::from(&projection.materialized_path);
        if !live_path.exists() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("projection path '{}' does not exist", live_path.display()),
            ));
        }

        let mut source_backup = None;
        if projection.method != "symlink" {
            let tmp_path = self
                .ctx
                .state_dir
                .join(format!("tmp-capture-{}", Uuid::new_v4()));
            let _ = remove_path_if_exists(&tmp_path);
            copy_dir_recursive(&live_path, &tmp_path).map_err(map_io)?;
            source_backup = backup_path_if_exists(&self.ctx, &skill_path, "capture.replace_source")
                .map_err(map_io)?;
            remove_path_if_exists(&skill_path).map_err(map_io)?;
            fs::rename(&tmp_path, &skill_path).map_err(map_io)?;
        }

        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;
        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        let commit = if changed {
            let message = args.message.clone().unwrap_or_else(|| {
                format!(
                    "capture({}): from {}",
                    projection.skill_id, projection.instance_id
                )
            });
            Some(gitops::commit(&self.ctx, &message).map_err(map_git)?)
        } else {
            None
        };
        let current_rev = gitops::head(&self.ctx).map_err(map_git)?;

        let mut projections = snapshot.projections;
        update_projection_after_capture(&mut projections, &projection.instance_id, &current_rev)?;
        paths.save_projections(&projections).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "skill.capture",
            json!({
                "skill_id": projection.skill_id,
                "binding_id": projection.binding_id,
                "instance_id": projection.instance_id,
                "request_id": request_id
            }),
            json!({
                "instance_id": projection.instance_id,
                "commit": commit
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({
                "capture": {
                    "skill_id": projection.skill_id,
                    "binding_id": projection.binding_id,
                    "instance_id": projection.instance_id,
                    "commit": commit,
                    "backup": source_backup,
                    "noop": !changed
                }
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_save(
        &self,
        args: &SaveArgs,
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

        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;
        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        if !changed {
            return Ok((json!({"skill": args.skill, "noop": true}), Meta::default()));
        }

        let message = args
            .message
            .clone()
            .unwrap_or_else(|| format!("save({}): event", args.skill));
        let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;
        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "save",
            request_id,
            json!({"skill": args.skill, "commit": commit}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "commit": commit, "noop": false}),
            meta,
        ))
    }

    pub fn cmd_snapshot(
        &self,
        args: &SkillOnlyArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let short = gitops::short_head(&self.ctx).map_err(map_git)?;
        let ts = Utc::now().format("%Y%m%dT%H%M%S%fZ");
        let tag = format!("snapshot/{}/{}-{}", args.skill, ts, short);
        gitops::create_annotated_tag(&self.ctx, &tag, &format!("snapshot {}", args.skill))
            .map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "snapshot",
            request_id,
            json!({"skill": args.skill, "tag": tag}),
            &mut meta,
        )?;

        Ok((json!({"skill": args.skill, "tag": tag}), meta))
    }

    pub fn cmd_release(
        &self,
        args: &ReleaseArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let tag = format!("release/{}/{}", args.skill, args.version);
        gitops::create_annotated_tag(
            &self.ctx,
            &tag,
            &format!("release {} {}", args.skill, args.version),
        )
        .map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "release",
            request_id,
            json!({"skill": args.skill, "tag": tag}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "version": args.version, "tag": tag}),
            meta,
        ))
    }

    pub fn cmd_rollback(
        &self,
        args: &RollbackArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        if args.to.is_some() && args.steps.is_some() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--to and --steps are mutually exclusive",
            ));
        }

        let reference = match (&args.to, args.steps) {
            (Some(r), _) => r.clone(),
            (None, Some(n)) => format!("HEAD~{}", n),
            (None, None) => "HEAD~1".to_string(),
        };

        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;
        gitops::resolve_ref(&self.ctx, &reference).map_err(map_git)?;

        let skill_rel = format!("skills/{}", args.skill);
        gitops::checkout_path_from_ref(&self.ctx, &reference, Path::new(&skill_rel))
            .map_err(map_git)?;
        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;

        let changed = gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel))
            .map_err(map_git)?;
        if !changed {
            return Ok((
                json!({"skill": args.skill, "reference": reference, "noop": true}),
                Meta::default(),
            ));
        }

        let message = format!("rollback({}): restore from {}", args.skill, reference);
        let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "rollback",
            request_id,
            json!({"skill": args.skill, "commit": commit, "reference": reference}),
            &mut meta,
        )?;

        Ok((
            json!({"skill": args.skill, "reference": reference, "commit": commit, "noop": false}),
            meta,
        ))
    }

    pub fn cmd_diff(
        &self,
        args: &DiffArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let skill_rel = format!("skills/{}", args.skill);
        let diff = gitops::diff_path(&self.ctx, &args.from, &args.to, Path::new(&skill_rel))
            .map_err(map_git)?;
        Ok((
            json!({"skill": args.skill, "from": args.from, "to": args.to, "diff": diff}),
            Meta::default(),
        ))
    }

    pub fn cmd_status(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let skill_inventory = collect_skill_inventory(&self.ctx);
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let pending_ops = pending_report.ops.len();
        let target_dirs = resolve_agent_skill_dirs(&self.ctx.root);
        let v3_paths = V3StatePaths::from_root(&self.ctx.root);
        let v3_status = v3_paths
            .maybe_load_snapshot()
            .map_err(map_v3_state)?
            .map(|snapshot| snapshot.status_view())
            .unwrap_or_else(|| {
                json!({
                    "state_model": "v3",
                    "available": false,
                    "error": {
                        "code": "STATE_CORRUPT",
                        "message": format!("v3 state not initialized under {}", v3_paths.v3_dir.display())
                    }
                })
            });
        let (registered_target_count, registered_target_ids) = v3_status
            .get("targets")
            .and_then(|value| value.as_array())
            .map(|targets| {
                let ids = targets
                    .iter()
                    .filter_map(|target| target.get("target_id").and_then(|id| id.as_str()))
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>();
                (targets.len(), ids)
            })
            .unwrap_or((0, Vec::new()));
        let mut git_warnings = Vec::new();
        let head = read_git_field(&self.ctx, &["rev-parse", "HEAD"], &mut git_warnings);
        let branch = read_git_field(
            &self.ctx,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            &mut git_warnings,
        );
        let status_short = read_git_field(&self.ctx, &["status", "--short"], &mut git_warnings);

        let (remote, mut meta) = remote_status_payload_with_pending(&self.ctx, pending_report)?;
        meta.warnings.splice(0..0, git_warnings);
        meta.warnings.extend(skill_inventory.warnings);

        let data = json!({
            "state_model": "v3",
            "skills": skill_inventory.source_skills,
            "backup_skills": skill_inventory.backup_skills,
            "skill_sources": {
                "dirs": skill_inventory
                    .source_dirs
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
                "count": skill_inventory.source_dirs.len(),
            },
            "backup_dir": self.ctx.skills_dir.display().to_string(),
            "git": {"head": head, "branch": branch, "status_short": status_short},
            "agent_dir_defaults": {
                "claude_dir": target_dirs.claude.display().to_string(),
                "codex_dir": target_dirs.codex.display().to_string()
            },
            "registered_targets": {
                "count": registered_target_count,
                "target_ids": registered_target_ids
            },
            "remote": remote,
            "pending_ops": pending_ops,
            "v3": v3_status
        });

        Ok((data, meta))
    }

    pub fn cmd_doctor(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let fsck = gitops::fsck(&self.ctx);
        let fsck_ok = fsck.is_ok();
        let fsck_output = fsck.unwrap_or_else(|e| e.to_string());
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let v3_paths = V3StatePaths::from_root(&self.ctx.root);
        let v3_schema_ok = v3_paths.schema_file.exists();
        let v3_snapshot_ok = v3_paths
            .maybe_load_snapshot()
            .map_err(map_v3_state)?
            .is_some();
        let history = gitops::history_status(&self.ctx).map_err(map_git)?;

        let healthy = fsck_ok && v3_schema_ok && v3_snapshot_ok && history.conflicts.is_empty();

        Ok((
            json!({
                "healthy": healthy,
                "checks": {
                    "git_fsck": {"ok": fsck_ok, "output": fsck_output},
                    "v3_schema_file": {"ok": v3_schema_ok},
                    "v3_snapshot": {"ok": v3_snapshot_ok},
                    "pending_queue": {
                        "count": pending_report.ops.len(),
                        "journal_events": pending_report.journal_events,
                        "history_events": pending_report.history_events,
                        "warnings": pending_report.warnings
                    },
                    "history_branch": history
                }
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_workspace_binding(
        &self,
        command: &WorkspaceBindingCommand,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            WorkspaceBindingCommand::Add(args) => self.cmd_workspace_binding_add(args, request_id),
            WorkspaceBindingCommand::List => Ok((
                {
                    let snapshot = self.require_v3_snapshot()?;
                    json!({
                        "state_model": "v3",
                        "count": snapshot.bindings.bindings.len(),
                        "bindings": snapshot.bindings.bindings
                    })
                },
                Meta::default(),
            )),
            WorkspaceBindingCommand::Show(args) => {
                let snapshot = self.require_v3_snapshot()?;
                let binding = snapshot.binding(&args.binding_id).cloned().ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::BindingNotFound,
                        format!("binding '{}' not found", args.binding_id),
                    )
                })?;
                let default_target = snapshot.binding_default_target(&binding);
                let rules = snapshot.binding_rules(&binding.binding_id);
                let projections = snapshot.binding_projections(&binding.binding_id);

                Ok((
                    json!({
                        "state_model": "v3",
                        "binding": binding,
                        "default_target": default_target,
                        "rules": rules,
                        "projections": projections
                    }),
                    Meta::default(),
                ))
            }
            WorkspaceBindingCommand::Remove(args) => {
                self.cmd_workspace_binding_remove(args, request_id)
            }
        }
    }

    pub fn cmd_target(
        &self,
        command: &TargetCommand,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            TargetCommand::Add(args) => self.cmd_target_add(args, request_id),
            TargetCommand::List => Ok((
                {
                    let snapshot = self.require_v3_snapshot()?;
                    json!({
                        "state_model": "v3",
                        "count": snapshot.targets.targets.len(),
                        "targets": snapshot.targets.targets
                    })
                },
                Meta::default(),
            )),
            TargetCommand::Show(args) => {
                let snapshot = self.require_v3_snapshot()?;
                let target = snapshot.target(&args.target_id).ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::TargetNotFound,
                        format!("target '{}' not found", args.target_id),
                    )
                })?;
                let relations = snapshot.target_relations(&target.target_id);

                Ok((
                    json!({
                        "state_model": "v3",
                        "target": target,
                        "bindings": relations.bindings,
                        "rules": relations.rules,
                        "projections": relations.projections
                    }),
                    Meta::default(),
                ))
            }
            TargetCommand::Remove(args) => self.cmd_target_remove(args, request_id),
        }
    }

    fn cmd_target_add(
        &self,
        args: &TargetAddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let target_path = PathBuf::from(&args.path);
        if !target_path.is_absolute() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--path must be absolute",
            ));
        }

        match args.ownership {
            TargetOwnership::Managed => fs::create_dir_all(&target_path).map_err(map_io)?,
            TargetOwnership::Observed | TargetOwnership::External => {
                if !target_path.exists() {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        format!(
                            "target path '{}' must exist for ownership '{}'",
                            target_path.display(),
                            target_ownership_as_str(args.ownership)
                        ),
                    ));
                }
            }
        }

        let paths = self.ensure_v3_layout()?;
        let mut targets = paths.load_targets().map_err(map_v3_state)?;

        if let Some(existing) = targets
            .targets
            .iter()
            .find(|target| {
                target.agent == agent_kind_as_str(args.agent) && target.path == args.path
            })
            .cloned()
        {
            if existing.ownership != target_ownership_as_str(args.ownership) {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "target '{}' already exists with ownership '{}'",
                        existing.target_id, existing.ownership
                    ),
                ));
            }
            return Ok((json!({"target": existing, "noop": true}), Meta::default()));
        }

        let target_id = unique_target_id(&targets, args);
        let target = V3ProjectionTarget {
            target_id: target_id.clone(),
            agent: agent_kind_as_str(args.agent).to_string(),
            path: args.path.clone(),
            ownership: target_ownership_as_str(args.ownership).to_string(),
            capabilities: target_capabilities(args.ownership),
            created_at: Some(Utc::now()),
        };

        targets.targets.push(target.clone());
        targets
            .targets
            .sort_by(|left, right| left.target_id.cmp(&right.target_id));
        paths.save_targets(&targets).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "target.add",
            json!({
                "target_id": target.target_id,
                "agent": target.agent,
                "path": target.path,
                "ownership": target.ownership,
                "request_id": request_id
            }),
            json!({
                "target_id": target.target_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({"target": target, "noop": false}),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    fn cmd_workspace_binding_add(
        &self,
        args: &BindingAddArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        validate_non_empty("profile", &args.profile)?;
        validate_non_empty("matcher_value", &args.matcher_value)?;
        validate_non_empty("target", &args.target)?;
        validate_non_empty("policy_profile", &args.policy_profile)?;

        let paths = self.ensure_v3_layout()?;
        let snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        if snapshot.target(&args.target).is_none() {
            return Err(CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", args.target),
            ));
        }

        if let Some(existing) = snapshot
            .bindings
            .bindings
            .iter()
            .find(|binding| {
                binding.agent == agent_kind_as_str(args.agent)
                    && binding.profile_id == args.profile
                    && binding.workspace_matcher.kind
                        == workspace_matcher_kind_as_str(args.matcher_kind)
                    && binding.workspace_matcher.value == args.matcher_value
                    && binding.default_target_id == args.target
                    && binding.policy_profile == args.policy_profile
            })
            .cloned()
        {
            return Ok((json!({"binding": existing, "noop": true}), Meta::default()));
        }

        let mut bindings = snapshot.bindings;
        let binding_id = unique_binding_id(&bindings, args);
        let binding = V3WorkspaceBinding {
            binding_id: binding_id.clone(),
            agent: agent_kind_as_str(args.agent).to_string(),
            profile_id: args.profile.clone(),
            workspace_matcher: V3WorkspaceMatcher {
                kind: workspace_matcher_kind_as_str(args.matcher_kind).to_string(),
                value: args.matcher_value.clone(),
            },
            default_target_id: args.target.clone(),
            policy_profile: args.policy_profile.clone(),
            active: true,
            created_at: Some(Utc::now()),
        };

        bindings.bindings.push(binding.clone());
        bindings
            .bindings
            .sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        paths.save_bindings(&bindings).map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "workspace.binding.add",
            json!({
                "binding_id": binding.binding_id,
                "agent": binding.agent,
                "profile_id": binding.profile_id,
                "matcher_kind": binding.workspace_matcher.kind,
                "matcher_value": binding.workspace_matcher.value,
                "target_id": binding.default_target_id,
                "request_id": request_id
            }),
            json!({
                "binding_id": binding.binding_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({"binding": binding, "noop": false}),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    fn cmd_workspace_binding_remove(
        &self,
        args: &crate::cli::BindingShowArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let paths = self.ensure_v3_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let binding = snapshot.binding(&args.binding_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{}' not found", args.binding_id),
            )
        })?;

        let removed_rules = snapshot.binding_rules(&args.binding_id);
        let removed_projections = snapshot.binding_projections(&args.binding_id);
        let orphaned_paths = removed_projections
            .iter()
            .map(|projection| projection.materialized_path.clone())
            .filter(|path| Path::new(path).exists())
            .collect::<Vec<_>>();

        snapshot
            .bindings
            .bindings
            .retain(|item| item.binding_id != args.binding_id);
        snapshot
            .rules
            .rules
            .retain(|item| item.binding_id != args.binding_id);
        snapshot
            .projections
            .projections
            .retain(|item| item.binding_id != args.binding_id);

        paths
            .save_bindings(&snapshot.bindings)
            .map_err(map_v3_state)?;
        paths.save_rules(&snapshot.rules).map_err(map_v3_state)?;
        paths
            .save_projections(&snapshot.projections)
            .map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "workspace.binding.remove",
            json!({
                "binding_id": binding.binding_id,
                "request_id": request_id
            }),
            json!({
                "binding_id": binding.binding_id,
                "removed_rules": removed_rules.iter().map(|rule| rule.skill_id.clone()).collect::<Vec<_>>(),
                "removed_projection_ids": removed_projections.iter().map(|projection| projection.instance_id.clone()).collect::<Vec<_>>(),
            }),
        )
        .map_err(map_v3_state)?;

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        if !orphaned_paths.is_empty() {
            meta.warnings.push(format!(
                "binding removed from state; {} live projection path(s) were left in place",
                orphaned_paths.len()
            ));
        }

        Ok((
            json!({
                "binding": binding,
                "removed_rules": removed_rules,
                "removed_projections": removed_projections,
                "orphaned_paths": orphaned_paths,
                "noop": false
            }),
            meta,
        ))
    }

    fn cmd_target_remove(
        &self,
        args: &crate::cli::TargetShowArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let paths = self.ensure_v3_layout()?;
        let mut snapshot = paths.load_snapshot().map_err(map_v3_state)?;
        let target = snapshot.target(&args.target_id).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::TargetNotFound,
                format!("target '{}' not found", args.target_id),
            )
        })?;

        let relations = snapshot.target_relations(&args.target_id);
        if !relations.bindings.is_empty()
            || !relations.rules.is_empty()
            || !relations.projections.is_empty()
        {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                format!(
                    "target '{}' is still referenced; remove dependent bindings or projections first",
                    args.target_id
                ),
            );
            failure.details = json!({
                "binding_ids": relations.bindings.iter().map(|binding| binding.binding_id.clone()).collect::<Vec<_>>(),
                "rule_skills": relations.rules.iter().map(|rule| rule.skill_id.clone()).collect::<Vec<_>>(),
                "projection_ids": relations.projections.iter().map(|projection| projection.instance_id.clone()).collect::<Vec<_>>(),
            });
            return Err(failure);
        }

        snapshot
            .targets
            .targets
            .retain(|item| item.target_id != args.target_id);
        paths
            .save_targets(&snapshot.targets)
            .map_err(map_v3_state)?;

        let op_id = record_v3_operation(
            &paths,
            "target.remove",
            json!({
                "target_id": target.target_id,
                "request_id": request_id
            }),
            json!({
                "target_id": target.target_id
            }),
        )
        .map_err(map_v3_state)?;

        Ok((
            json!({
                "target": target,
                "noop": false
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_remote(
        &self,
        command: &RemoteCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            RemoteCommand::Set { url } => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                gitops::set_remote_origin(&self.ctx, url).map_err(map_git)?;
                Ok((json!({"remote": "origin", "url": url}), Meta::default()))
            }
            RemoteCommand::Status => {
                let (remote, meta) = remote_status_payload(&self.ctx)?;
                Ok((json!({"remote": remote}), meta))
            }
        }
    }

    fn require_v3_snapshot(&self) -> std::result::Result<V3Snapshot, CommandFailure> {
        let paths = V3StatePaths::from_root(&self.ctx.root);
        match paths.maybe_load_snapshot().map_err(map_v3_state)? {
            Some(snapshot) => Ok(snapshot),
            None => Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("v3 state not initialized under {}", paths.v3_dir.display()),
            )),
        }
    }

    fn ensure_v3_layout(&self) -> std::result::Result<V3StatePaths, CommandFailure> {
        let paths = V3StatePaths::from_root(&self.ctx.root);
        paths.ensure_layout().map_err(map_v3_state)?;
        Ok(paths)
    }

    pub fn cmd_sync(
        &self,
        command: &SyncCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            SyncCommand::Status => {
                let (remote, meta) = remote_status_payload(&self.ctx)?;
                Ok((json!({"remote": remote}), meta))
            }
            SyncCommand::Push => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let res = sync_push_internal(&self.ctx)?;
                Ok((json!({"result": res}), Meta::default()))
            }
            SyncCommand::Pull => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                if !gitops::remote_exists(&self.ctx) {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        "remote origin not configured",
                    ));
                }
                if !gitops::fetch_origin_main_if_present(&self.ctx)
                    .map_err(map_remote_unreachable)?
                {
                    return Ok((
                        json!({"result": "remote_empty", "replay": "no_pending_ops"}),
                        Meta::default(),
                    ));
                }
                let history_fetch = gitops::fetch_origin_history_branch_if_present(&self.ctx);
                gitops::pull_rebase_main(&self.ctx).map_err(map_replay_conflict)?;
                let replay = sync_replay_internal(&self.ctx)?;
                let mut meta = Meta::default();
                match history_fetch {
                    Ok(true) => {
                        if let Some(warning) =
                            gitops::sync_history_branch_from_remote(&self.ctx).map_err(map_git)?
                        {
                            meta.warnings.push(warning);
                        }
                    }
                    Ok(false) => {}
                    Err(err) => meta.warnings.push(format!(
                        "failed to fetch origin/{}: {}",
                        gitops::HISTORY_BRANCH,
                        err
                    )),
                }
                Ok((json!({"result": "pulled", "replay": replay}), meta))
            }
            SyncCommand::Replay => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let replay = sync_replay_internal(&self.ctx)?;
                Ok((json!({"result": replay}), Meta::default()))
            }
        }
    }

    pub fn cmd_ops(
        &self,
        command: &OpsCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            OpsCommand::List => {
                let report = self.ctx.read_pending_report().map_err(map_io)?;
                Ok((
                    json!({
                        "count": report.ops.len(),
                        "ops": report.ops,
                        "journal_events": report.journal_events,
                        "history_events": report.history_events
                    }),
                    Meta {
                        warnings: report.warnings,
                        sync_state: None,
                        op_id: None,
                    },
                ))
            }
            OpsCommand::Retry => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let pending_before = self.ctx.pending_count().map_err(map_io)?;
                let result = sync_replay_internal(&self.ctx)?;
                let pending_after = self.ctx.pending_count().map_err(map_io)?;
                Ok((
                    json!({
                        "result": result,
                        "pending_before": pending_before,
                        "pending_after": pending_after
                    }),
                    Meta::default(),
                ))
            }
            OpsCommand::Purge => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_layout()?;
                let purged = self.ctx.purge_pending().map_err(map_io)?;
                Ok((json!({"purged": purged}), Meta::default()))
            }
            OpsCommand::History { command } => self.cmd_ops_history(command),
        }
    }

    fn cmd_ops_history(
        &self,
        command: &OpsHistoryCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            OpsHistoryCommand::Diagnose => {
                let report = gitops::history_status(&self.ctx).map_err(map_git)?;
                Ok((json!(report), Meta::default()))
            }
            OpsHistoryCommand::Repair(args) => {
                let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
                self.ensure_write_repo_ready()?;
                let strategy = match args.strategy {
                    HistoryRepairStrategyArg::Local => gitops::HistoryRepairStrategy::Local,
                    HistoryRepairStrategyArg::Remote => gitops::HistoryRepairStrategy::Remote,
                };
                let report = gitops::repair_history_branch(&self.ctx, strategy).map_err(map_git)?;
                Ok((json!(report), Meta::default()))
            }
        }
    }
}

fn ensure_initial_commit(ctx: &AppContext) -> Result<()> {
    if gitops::head(ctx).is_ok() {
        return Ok(());
    }
    gitops::run_git(
        ctx,
        &[
            "commit",
            "--allow-empty",
            "-m",
            "chore: initialize skill registry",
        ],
    )?;
    Ok(())
}

fn ensure_skill_exists(ctx: &AppContext, skill: &str) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    if !ctx.skill_path(skill).exists() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    Ok(())
}

fn validate_skill_name(skill: &str) -> Result<()> {
    if skill.is_empty() {
        return Err(anyhow!("skill name cannot be empty"));
    }
    if skill == "." || skill == ".." {
        return Err(anyhow!("skill name cannot be '.' or '..'"));
    }
    if skill
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
    {
        return Err(anyhow!(
            "skill name '{}' contains unsupported characters; use [A-Za-z0-9._-]",
            skill
        ));
    }
    Ok(())
}

fn read_git_field(ctx: &AppContext, args: &[&str], warnings: &mut Vec<String>) -> Option<String> {
    match gitops::run_git(ctx, args) {
        Ok(value) if value.is_empty() => None,
        Ok(value) => Some(value),
        Err(err) => {
            warnings.push(format!("git {:?} unavailable: {}", args, err));
            None
        }
    }
}

fn rollback_added_skill(ctx: &AppContext, skill_rel: &str, dst: &Path) {
    let _ = remove_path_if_exists(dst);
    let _ = gitops::run_git_allow_failure(ctx, &["reset", "HEAD", "--", skill_rel]);
}

fn backup_path_if_exists(
    ctx: &AppContext,
    path: &Path,
    reason: &str,
) -> Result<Option<serde_json::Value>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to inspect path before backup: {}", path.display())
            });
        }
    };

    let ts = Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string();
    let entry = format!(
        "{}-{}-{}",
        slugify(reason),
        slugify(
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("path")
        ),
        Uuid::new_v4().simple()
    );
    let backup_root = ctx.state_dir.join("backups").join(ts);
    fs::create_dir_all(&backup_root)
        .with_context(|| format!("failed to create backup root {}", backup_root.display()))?;
    let backup_path = backup_root.join(entry);

    let kind = if metadata.file_type().is_symlink() {
        backup_symlink_metadata(path, &backup_path)?;
        "symlink"
    } else if metadata.is_dir() {
        copy_dir_recursive(path, &backup_path)?;
        "dir"
    } else {
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create backup parent {}", parent.display()))?;
        }
        fs::copy(path, &backup_path).with_context(|| {
            format!(
                "failed to copy file {} to {}",
                path.display(),
                backup_path.display()
            )
        })?;
        "file"
    };

    Ok(Some(json!({
        "reason": reason,
        "kind": kind,
        "original_path": path.display().to_string(),
        "backup_path": backup_path.display().to_string()
    })))
}

fn backup_symlink_metadata(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)
        .with_context(|| format!("failed to create symlink backup dir {}", dst.display()))?;
    let target = fs::read_link(src)
        .with_context(|| format!("failed to resolve symlink {}", src.display()))?;

    let payload = json!({
        "source": src.display().to_string(),
        "target": target.display().to_string()
    });
    let raw = serde_json::to_string_pretty(&payload)?;
    fs::write(dst.join("symlink.json"), raw + "\n").with_context(|| {
        format!(
            "failed to write symlink backup metadata for {}",
            src.display()
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("source does not exist: {}", src.display()));
    }
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in WalkDir::new(src).follow_links(true).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", src.display()))?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive_without_symlinks(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("source does not exist: {}", src.display()));
    }
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;

    for entry in WalkDir::new(src).follow_links(false).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", src.display()))?;
        let rel = entry.path().strip_prefix(src)?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        if entry.file_type().is_symlink() {
            return Err(anyhow!(
                "source contains unsupported symlink entry '{}'",
                rel.display()
            ));
        }

        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn create_symlink_dir(src: &Path, dst: &Path) -> Result<()> {
    std::os::unix::fs::symlink(src, dst).context("failed to create symlink")?;
    Ok(())
}

#[cfg(windows)]
fn create_symlink_dir(src: &Path, dst: &Path) -> Result<()> {
    std::os::windows::fs::symlink_dir(src, dst).context("failed to create symlink")?;
    Ok(())
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Workspace { command } => match command {
            WorkspaceCommand::Status => "workspace.status",
            WorkspaceCommand::Doctor => "workspace.doctor",
            WorkspaceCommand::Binding { command } => match command {
                WorkspaceBindingCommand::Add(_) => "workspace.binding.add",
                WorkspaceBindingCommand::List => "workspace.binding.list",
                WorkspaceBindingCommand::Show(_) => "workspace.binding.show",
                WorkspaceBindingCommand::Remove(_) => "workspace.binding.remove",
            },
            WorkspaceCommand::Remote { .. } => "workspace.remote",
        },
        Command::Target { command } => match command {
            TargetCommand::Add(_) => "target.add",
            TargetCommand::List => "target.list",
            TargetCommand::Show(_) => "target.show",
            TargetCommand::Remove(_) => "target.remove",
        },
        Command::Skill { command } => match command {
            SkillCommand::Add(_) => "skill.add",
            SkillCommand::Project(_) => "skill.project",
            SkillCommand::Capture(_) => "skill.capture",
            SkillCommand::Save(_) => "skill.save",
            SkillCommand::Snapshot(_) => "skill.snapshot",
            SkillCommand::Release(_) => "skill.release",
            SkillCommand::Rollback(_) => "skill.rollback",
            SkillCommand::Diff(_) => "skill.diff",
        },
        Command::Sync { command } => match command {
            SyncCommand::Status => "sync.status",
            SyncCommand::Push => "sync.push",
            SyncCommand::Pull => "sync.pull",
            SyncCommand::Replay => "sync.replay",
        },
        Command::Ops { command } => match command {
            OpsCommand::List => "ops.list",
            OpsCommand::Retry => "ops.retry",
            OpsCommand::Purge => "ops.purge",
            OpsCommand::History { command } => match command {
                OpsHistoryCommand::Diagnose => "ops.history.diagnose",
                OpsHistoryCommand::Repair(_) => "ops.history.repair",
            },
        },
        Command::Panel(_) => "panel",
    }
}

fn agent_kind_as_str(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
    }
}

fn workspace_matcher_kind_as_str(kind: WorkspaceMatcherKind) -> &'static str {
    match kind {
        WorkspaceMatcherKind::PathPrefix => "path_prefix",
        WorkspaceMatcherKind::ExactPath => "exact_path",
        WorkspaceMatcherKind::Name => "name",
    }
}

fn target_ownership_as_str(ownership: TargetOwnership) -> &'static str {
    match ownership {
        TargetOwnership::Managed => "managed",
        TargetOwnership::Observed => "observed",
        TargetOwnership::External => "external",
    }
}

fn target_capabilities(ownership: TargetOwnership) -> V3TargetCapabilities {
    match ownership {
        TargetOwnership::Managed => V3TargetCapabilities {
            symlink: true,
            copy: true,
            watch: true,
        },
        TargetOwnership::Observed => V3TargetCapabilities {
            symlink: false,
            copy: false,
            watch: true,
        },
        TargetOwnership::External => V3TargetCapabilities {
            symlink: false,
            copy: false,
            watch: false,
        },
    }
}

fn projection_method_as_str(method: ProjectionMethod) -> &'static str {
    match method {
        ProjectionMethod::Symlink => "symlink",
        ProjectionMethod::Copy => "copy",
        ProjectionMethod::Materialize => "materialize",
    }
}

fn validate_non_empty(name: &str, value: &str) -> std::result::Result<(), CommandFailure> {
    if value.trim().is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("--{} must not be empty", name),
        ));
    }
    Ok(())
}

fn validate_projection_method(
    target: &V3ProjectionTarget,
    method: ProjectionMethod,
) -> std::result::Result<(), CommandFailure> {
    match method {
        ProjectionMethod::Symlink if !target.capabilities.symlink => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "target '{}' does not support symlink projections",
                target.target_id
            ),
        )),
        ProjectionMethod::Copy | ProjectionMethod::Materialize if !target.capabilities.copy => {
            Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' does not support copy/materialize projections",
                    target.target_id
                ),
            ))
        }
        _ => Ok(()),
    }
}

fn unique_target_id(targets: &V3TargetsFile, args: &TargetAddArgs) -> String {
    unique_target_id_for(args.agent, &args.path, targets)
}

fn unique_target_id_for(agent: AgentKind, path: &str, targets: &V3TargetsFile) -> String {
    let token = target_path_token(path, agent);
    let base = format!("target_{}_{}", agent_kind_as_str(agent), slugify(&token));
    unique_id(
        &base,
        targets
            .targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect(),
    )
}

fn target_path_token(path: &str, agent: AgentKind) -> String {
    let route = Path::new(path);
    let leaf = route
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(agent_kind_as_str(agent));

    // Names like ".../skills" are too generic. Include the parent to keep ids readable:
    // ".claude/skills" => "claude_skills", ".claude-work/skills" => "claude-work_skills".
    if (leaf.eq_ignore_ascii_case("skills") || leaf.eq_ignore_ascii_case("skill"))
        && let Some(parent) = route
            .parent()
            .and_then(|value| value.file_name())
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
    {
        return format!("{}_{}", parent, leaf);
    }

    leaf.to_string()
}

fn unique_binding_id(bindings: &V3BindingsFile, args: &BindingAddArgs) -> String {
    let matcher_token = binding_matcher_token(args);
    let base = format!(
        "bind_{}_{}",
        agent_kind_as_str(args.agent),
        slugify(&matcher_token)
    );
    unique_id(
        &base,
        bindings
            .bindings
            .iter()
            .map(|binding| binding.binding_id.as_str())
            .collect(),
    )
}

fn binding_matcher_token(args: &BindingAddArgs) -> String {
    match args.matcher_kind {
        WorkspaceMatcherKind::PathPrefix | WorkspaceMatcherKind::ExactPath => {
            Path::new(&args.matcher_value)
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or(&args.profile)
                .to_string()
        }
        WorkspaceMatcherKind::Name => args.matcher_value.clone(),
    }
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }

    let normalized = out.trim_matches('_');
    if normalized.is_empty() {
        "item".to_string()
    } else {
        normalized.to_string()
    }
}

fn unique_id(base: &str, existing: Vec<&str>) -> String {
    let existing = existing
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    if !existing.contains(base) {
        return base.to_string();
    }

    for index in 2..1000 {
        let candidate = format!("{}_{}", base, index);
        if !existing.contains(candidate.as_str()) {
            return candidate;
        }
    }

    format!("{}_{}", base, Uuid::new_v4().simple())
}

fn projection_instance_id(skill: &str, binding_id: &str, target_id: &str) -> String {
    format!(
        "inst_{}_{}_{}",
        slugify(skill),
        slugify(binding_id),
        slugify(target_id)
    )
}

fn upsert_rule(rules: &mut V3RulesFile, rule: V3BindingRule) {
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

fn upsert_projection(projections: &mut V3ProjectionsFile, projection: V3ProjectionInstance) {
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

fn project_skill_to_target(src: &Path, dst: &Path, method: ProjectionMethod) -> Result<()> {
    match method {
        ProjectionMethod::Symlink => create_symlink_dir(src, dst),
        ProjectionMethod::Copy | ProjectionMethod::Materialize => copy_dir_recursive(src, dst),
    }
}

fn resolve_capture_projection(
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

fn update_projection_after_capture(
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

fn record_v3_operation(
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

fn map_project_io(method: ProjectionMethod) -> impl FnOnce(anyhow::Error) -> CommandFailure {
    move |err| {
        CommandFailure::new(
            ErrorCode::IoError,
            format!(
                "failed to project skill using {}: {}",
                projection_method_as_str(method),
                err
            ),
        )
    }
}

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

pub fn remote_status_payload(
    ctx: &AppContext,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let pending_report = ctx.read_pending_report().map_err(map_io)?;
    remote_status_payload_with_pending(ctx, pending_report)
}

fn remote_status_payload_with_pending(
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

fn maybe_autosync_or_queue(
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

fn sync_push_internal(ctx: &AppContext) -> std::result::Result<&'static str, CommandFailure> {
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

fn sync_replay_internal(ctx: &AppContext) -> std::result::Result<&'static str, CommandFailure> {
    let pending = ctx.pending_count().map_err(map_io)?;
    if pending == 0 {
        return Ok("no_pending_ops");
    }
    sync_push_internal(ctx)?;
    Ok("replayed")
}

fn map_arg(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, err.to_string())
}

fn map_io<E: std::fmt::Display>(err: E) -> CommandFailure {
    CommandFailure::new(ErrorCode::IoError, err.to_string())
}

fn map_queue<E: std::fmt::Display>(err: E) -> CommandFailure {
    CommandFailure::new(ErrorCode::QueueBlocked, err.to_string())
}

fn map_git(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::GitError, err.to_string())
}

fn map_lock(err: anyhow::Error) -> CommandFailure {
    let message = err.to_string();
    if let Some(rest) = message.strip_prefix("ARG_INVALID:") {
        return CommandFailure::new(ErrorCode::ArgInvalid, rest.trim());
    }
    CommandFailure::new(ErrorCode::LockBusy, message)
}

fn map_remote_unreachable(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::RemoteUnreachable, err.to_string())
}

fn map_push_rejected(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::PushRejected, err.to_string())
}

fn map_replay_conflict(err: anyhow::Error) -> CommandFailure {
    CommandFailure::new(ErrorCode::ReplayConflict, err.to_string())
}

fn map_v3_state(err: anyhow::Error) -> CommandFailure {
    let message = err.to_string();
    if message.contains("schema version mismatch") {
        CommandFailure::new(ErrorCode::SchemaMismatch, message)
    } else {
        CommandFailure::new(ErrorCode::StateCorrupt, message)
    }
}
