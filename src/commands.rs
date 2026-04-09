use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::cli::{
    AddArgs, AgentKind, BindingAddArgs, CaptureArgs, Cli, Command, DiffArgs,
    HistoryRepairStrategyArg, ImportArgs, InitArgs, LinkArgs, MigrateCommand, MigrateV2ToV3Args,
    OpsCommand, OpsHistoryCommand, ProjectArgs, ProjectionMethod, ReleaseArgs, RemoteCommand,
    RollbackArgs, SaveArgs, SkillCommand, SkillOnlyArgs, SyncCommand, Target, TargetAddArgs,
    TargetCommand, TargetOwnership, WorkspaceBindingCommand, WorkspaceCommand,
    WorkspaceMatcherKind,
};
use crate::envelope::{Envelope, Meta};
use crate::gitops;
use crate::state::{AppContext, remove_path_if_exists, resolve_agent_skill_dirs};
use crate::types::{ErrorCode, SkillTargetConfig, SyncState};
use crate::v3::{
    V3BindingRule, V3BindingsFile, V3OperationRecord, V3ProjectionInstance, V3ProjectionTarget,
    V3ProjectionsFile, V3RulesFile, V3Snapshot, V3StatePaths, V3TargetCapabilities, V3TargetsFile,
    V3WorkspaceBinding, V3WorkspaceMatcher,
};

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

#[derive(Debug, Clone)]
struct InitResolved {
    from_agent: Target,
    target: Target,
    copy: bool,
    force: bool,
    skip_backup: bool,
    backup_dir: Option<String>,
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
                WorkspaceCommand::Init(args) => self.cmd_init(args, &request_id),
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
                SkillCommand::Import(args) => self.cmd_import(args, &request_id),
                SkillCommand::Project(args) => self.cmd_project(args, &request_id),
                SkillCommand::Capture(args) => self.cmd_capture(args, &request_id),
                SkillCommand::Link(args) | SkillCommand::Use(args) => self.cmd_link(args),
                SkillCommand::Save(args) => self.cmd_save(args, &request_id),
                SkillCommand::Snapshot(args) => self.cmd_snapshot(args, &request_id),
                SkillCommand::Release(args) => self.cmd_release(args, &request_id),
                SkillCommand::Rollback(args) => self.cmd_rollback(args, &request_id),
                SkillCommand::Diff(args) => self.cmd_diff(args),
            },
            Command::Sync { command } => self.cmd_sync(command),
            Command::Ops { command } => self.cmd_ops(command),
            Command::Migrate { command } => self.cmd_migrate(command, &request_id),
            Command::Panel(_) => Ok((json!({"message": "panel handled in main"}), Meta::default())),

            Command::LegacyInit(_) => self.unsupported_v1_command("init", "workspace init"),
            Command::LegacyAdd(_) => self.unsupported_v1_command("add", "skill add"),
            Command::LegacyImport(_) => self.unsupported_v1_command("import", "skill import"),
            Command::LegacyLink(_) => self.unsupported_v1_command("link", "skill link"),
            Command::LegacyUse(_) => self.unsupported_v1_command("use", "skill use"),
            Command::LegacySave(_) => self.unsupported_v1_command("save", "skill save"),
            Command::LegacySnapshot(_) => self.unsupported_v1_command("snapshot", "skill snapshot"),
            Command::LegacyRelease(_) => self.unsupported_v1_command("release", "skill release"),
            Command::LegacyRollback(_) => self.unsupported_v1_command("rollback", "skill rollback"),
            Command::LegacyDiff(_) => self.unsupported_v1_command("diff", "skill diff"),
            Command::LegacyStatus => self.unsupported_v1_command("status", "workspace status"),
            Command::LegacyDoctor => self.unsupported_v1_command("doctor", "workspace doctor"),
            Command::LegacyRemote { .. } => {
                self.unsupported_v1_command("remote", "workspace remote <set|status>")
            }
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

    pub fn cmd_init(
        &self,
        args: &InitArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let resolved = resolve_init_args(args).map_err(map_arg)?;

        let (backup, mut backup_warnings) = if resolved.skip_backup {
            (
                serde_json::Value::Null,
                vec!["backup skipped by flag".to_string()],
            )
        } else {
            backup_agent_skills(
                &self.ctx,
                resolved.from_agent,
                resolved.backup_dir.as_deref(),
            )
            .map_err(map_io)?
        };

        let import_args = ImportArgs {
            source: None,
            from_agent: Some(resolved.from_agent),
            skill: None,
            link: false,
            target: resolved.target,
            copy: resolved.copy,
            force: resolved.force,
        };

        let (import_data, mut import_meta) = self.import_impl(&import_args, request_id)?;

        let mut link_names = std::collections::BTreeSet::<String>::new();
        for item in import_data["imported"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            if let Some(name) = item["skill"].as_str() {
                link_names.insert(name.to_string());
            }
        }
        for item in import_data["skipped"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            if let Some(name) = item["skill"].as_str() {
                link_names.insert(name.to_string());
            }
        }

        let mut linked = Vec::new();
        let mut link_warnings = Vec::new();
        for skill in link_names {
            let link_args = LinkArgs {
                skill: skill.clone(),
                target: resolved.target,
                copy: resolved.copy,
            };
            let (links, mut warnings) = link_skill(&self.ctx, &link_args)?;
            link_warnings.append(&mut warnings);
            linked.push(json!({
                "skill": skill,
                "links": links
            }));
        }

        let imported_len = import_data["imported"]
            .as_array()
            .map(|arr| arr.len())
            .unwrap_or(0);
        let skipped_len = import_data["skipped"]
            .as_array()
            .map(|arr| arr.len())
            .unwrap_or(0);
        let summary = json!({
            "candidates": imported_len + skipped_len,
            "imported": imported_len,
            "skipped": skipped_len,
            "linked": linked.len(),
        });

        import_meta.warnings.append(&mut backup_warnings);
        import_meta.warnings.append(&mut link_warnings);

        Ok((
            json!({
                "options": {
                    "from_agent": target_as_str(resolved.from_agent),
                    "target": target_as_str(resolved.target),
                    "copy": resolved.copy,
                    "force": resolved.force,
                    "skip_backup": resolved.skip_backup,
                    "backup_dir": resolved.backup_dir
                },
                "backup": backup,
                "import": import_data,
                "linked": linked,
                "summary": summary
            }),
            import_meta,
        ))
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

        if Path::new(&args.source).exists() {
            copy_dir_recursive(Path::new(&args.source), &dst).map_err(map_io)?;
        } else {
            let tmp = self
                .ctx
                .state_dir
                .join(format!("tmp-add-{}", Uuid::new_v4()));
            let source = args.source.as_str();
            let clone = gitops::run_git_allow_failure(
                &self.ctx,
                &[
                    "clone",
                    "--depth",
                    "1",
                    source,
                    tmp.to_string_lossy().as_ref(),
                ],
            )
            .map_err(map_git)?;
            if !clone.status.success() {
                let stderr = String::from_utf8_lossy(&clone.stderr).to_string();
                let _ = remove_path_if_exists(&tmp);
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("failed to clone source: {}", stderr.trim()),
                ));
            }
            copy_dir_recursive(&tmp, &dst).map_err(map_io)?;
            let _ = remove_path_if_exists(&tmp);
        }

        let mut meta = Meta::default();
        let skill_rel = format!("skills/{}", args.name);
        gitops::stage_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)?;
        if gitops::has_staged_changes_for_path(&self.ctx, Path::new(&skill_rel)).map_err(map_git)? {
            let message = format!("add({}): import {}", args.name, args.source);
            let commit = gitops::commit(&self.ctx, &message).map_err(map_git)?;
            maybe_autosync_or_queue(
                &self.ctx,
                "add",
                request_id,
                json!({"skill": args.name, "commit": commit}),
                &mut meta,
            )?;
        }

        Ok((json!({"skill": args.name, "path": dst}), meta))
    }

    pub fn cmd_import(
        &self,
        args: &ImportArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.import_impl(args, request_id)
    }

    fn import_impl(
        &self,
        args: &ImportArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let (candidates, mut warnings) = collect_import_candidates(args).map_err(map_arg)?;
        if candidates.is_empty() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "no skills found to import",
            ));
        }

        let mut imported = Vec::new();
        let mut skipped = Vec::new();

        for candidate in candidates {
            let dst = self.ctx.skill_path(&candidate.skill);
            if dst.exists() && !args.force {
                skipped.push(json!({
                    "skill": candidate.skill,
                    "reason": "already_exists",
                    "source": candidate.source,
                }));
                continue;
            }

            if dst.exists() {
                remove_path_if_exists(&dst).map_err(map_io)?;
            }
            copy_dir_recursive(&candidate.source, &dst).map_err(map_io)?;
            imported.push(json!({
                "skill": candidate.skill,
                "source": candidate.source,
                "origin": candidate.origin,
                "destination": dst,
            }));
        }

        if imported.is_empty() {
            return Ok((
                json!({
                    "imported": [],
                    "skipped": skipped,
                    "linked": [],
                    "commit": serde_json::Value::Null,
                    "summary": {
                        "candidates": 0,
                        "imported": 0,
                        "skipped": skipped.len(),
                        "linked": 0
                    }
                }),
                Meta {
                    warnings,
                    sync_state: None,
                    op_id: None,
                },
            ));
        }

        gitops::stage_path(&self.ctx, Path::new("skills")).map_err(map_git)?;
        let mut meta = Meta::default();
        let mut commit = None;
        if gitops::has_staged_changes_for_path(&self.ctx, Path::new("skills")).map_err(map_git)? {
            let message = format!("import: {} skill(s)", imported.len());
            let sha = gitops::commit(&self.ctx, &message).map_err(map_git)?;
            maybe_autosync_or_queue(
                &self.ctx,
                "import",
                request_id,
                json!({
                    "skills": imported
                        .iter()
                        .filter_map(|item| item["skill"].as_str())
                        .collect::<Vec<_>>(),
                    "commit": sha
                }),
                &mut meta,
            )?;
            commit = Some(sha);
        }

        let mut linked = Vec::new();
        if args.link {
            let skill_names: Vec<String> = imported
                .iter()
                .filter_map(|item| item["skill"].as_str().map(|s| s.to_string()))
                .collect();
            for skill in skill_names {
                let link_args = LinkArgs {
                    skill: skill.clone(),
                    target: args.target,
                    copy: args.copy,
                };
                let (links, mut link_warnings) = link_skill(&self.ctx, &link_args)?;
                warnings.append(&mut link_warnings);
                linked.push(json!({
                    "skill": skill,
                    "links": links
                }));
            }
        }

        meta.warnings.extend(warnings);

        Ok((
            json!({
                "imported": imported,
                "skipped": skipped,
                "linked": linked,
                "commit": commit,
                "summary": {
                    "candidates": imported.len() + skipped.len(),
                    "imported": imported.len(),
                    "skipped": skipped.len(),
                    "linked": linked.len()
                }
            }),
            meta,
        ))
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
            json!({"projection": projection, "noop": false}),
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

        if projection.method != "symlink" {
            let tmp_path = self
                .ctx
                .state_dir
                .join(format!("tmp-capture-{}", Uuid::new_v4()));
            let _ = remove_path_if_exists(&tmp_path);
            copy_dir_recursive(&live_path, &tmp_path).map_err(map_io)?;
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
                    "noop": !changed
                }
            }),
            Meta {
                op_id: Some(op_id),
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_link(
        &self,
        args: &LinkArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        self.link_impl(args)
    }

    fn link_impl(
        &self,
        args: &LinkArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let (linked, warnings) = link_skill(&self.ctx, args)?;

        Ok((
            json!({"skill": args.skill, "links": linked}),
            Meta {
                warnings,
                sync_state: None,
                op_id: None,
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
        let skills = list_skills(&self.ctx).map_err(map_io)?;
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let target_dirs = resolve_agent_skill_dirs();
        let v3_paths = V3StatePaths::from_root(&self.ctx.root);
        let v3_status = v3_paths.maybe_load_snapshot().map_err(map_v3_state)?;
        let mut warnings = pending_report.warnings;
        let head = read_git_field(&self.ctx, &["rev-parse", "HEAD"], &mut warnings);
        let branch = read_git_field(
            &self.ctx,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            &mut warnings,
        );
        let status_short = read_git_field(&self.ctx, &["status", "--short"], &mut warnings);

        let (remote, mut meta) = remote_status_payload(&self.ctx)?;
        meta.warnings.splice(0..0, warnings);

        let mut data = json!({
            "state_model": if v3_status.is_some() { "v3" } else { "v2" },
            "skills": skills,
            "git": {"head": head, "branch": branch, "status_short": status_short},
            "targets": {
                "claude_dir": target_dirs.claude.display().to_string(),
                "codex_dir": target_dirs.codex.display().to_string()
            },
            "remote": remote,
            "pending_ops": pending_report.ops.len()
        });

        if let Some(snapshot) = v3_status {
            data["v3"] = snapshot.status_view();
        }

        Ok((data, meta))
    }

    pub fn cmd_doctor(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let fsck = gitops::fsck(&self.ctx);
        let fsck_ok = fsck.is_ok();
        let fsck_output = fsck.unwrap_or_else(|e| e.to_string());
        let pending_report = self.ctx.read_pending_report().map_err(map_io)?;
        let targets_ok = self.ctx.load_targets().is_ok();
        let history = gitops::history_status(&self.ctx).map_err(map_git)?;

        let healthy = fsck_ok && targets_ok && history.conflicts.is_empty();

        Ok((
            json!({
                "healthy": healthy,
                "checks": {
                    "git_fsck": {"ok": fsck_ok, "output": fsck_output},
                    "targets_file": {"ok": targets_ok},
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
                let target = snapshot.target(&args.target_id).cloned().ok_or_else(|| {
                    CommandFailure::new(
                        ErrorCode::TargetNotFound,
                        format!("target '{}' not found", args.target_id),
                    )
                })?;
                let rules = snapshot.target_rules(&target.target_id);
                let projections = snapshot.target_projections(&target.target_id);
                let bindings = snapshot.target_bindings(&target.target_id);

                Ok((
                    json!({
                        "state_model": "v3",
                        "target": target,
                        "bindings": bindings,
                        "rules": rules,
                        "projections": projections
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

        let bindings = snapshot.target_bindings(&args.target_id);
        let rules = snapshot.target_rules(&args.target_id);
        let projections = snapshot.target_projections(&args.target_id);
        if !bindings.is_empty() || !rules.is_empty() || !projections.is_empty() {
            let mut failure = CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "target '{}' is still referenced; remove dependent bindings or projections first",
                    args.target_id
                ),
            );
            failure.details = json!({
                "binding_ids": bindings.iter().map(|binding| binding.binding_id.clone()).collect::<Vec<_>>(),
                "rule_skills": rules.iter().map(|rule| rule.skill_id.clone()).collect::<Vec<_>>(),
                "projection_ids": projections.iter().map(|projection| projection.instance_id.clone()).collect::<Vec<_>>(),
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

    fn cmd_migrate(
        &self,
        command: &MigrateCommand,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            MigrateCommand::V2ToV3(args) => self.cmd_migrate_v2_to_v3(args, request_id),
        }
    }

    fn cmd_migrate_v2_to_v3(
        &self,
        args: &MigrateV2ToV3Args,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let plan = self.build_v2_to_v3_migration_plan().map_err(map_io)?;
        if !args.apply {
            return Ok((
                json!({ "migration": plan.as_json("plan") }),
                Meta::default(),
            ));
        }

        if !plan.unresolved.is_empty() {
            let mut failure = CommandFailure::new(
                ErrorCode::ArgInvalid,
                "migration has unresolved legacy targets; run --plan and fix them before --apply",
            );
            failure.details = json!({ "migration": plan.as_json("apply") });
            return Err(failure);
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        let paths = V3StatePaths::from_root(&self.ctx.root);
        let mut snapshot = paths.load_or_init_snapshot().map_err(map_io)?;
        let mut meta = Meta::default();
        let now = Utc::now();
        let before_count = snapshot.targets.targets.len();
        let mut created = Vec::new();
        let mut skipped = Vec::new();

        for candidate in &plan.candidate_targets {
            if candidate.existing_target_id.is_some() {
                skipped.push(json!({
                    "target_id": candidate.target_id,
                    "path": candidate.path,
                    "reason": "already_exists"
                }));
                continue;
            }

            snapshot.targets.targets.push(V3ProjectionTarget {
                target_id: candidate.target_id.clone(),
                agent: agent_kind_as_str(candidate.agent).to_string(),
                path: candidate.path.clone(),
                ownership: target_ownership_as_str(TargetOwnership::Observed).to_string(),
                capabilities: target_capabilities(TargetOwnership::Observed),
                created_at: Some(now),
            });
            created.push(json!({
                "target_id": candidate.target_id,
                "agent": agent_kind_as_str(candidate.agent),
                "path": candidate.path,
                "ownership": "observed",
                "source_skills": candidate.source_skills
            }));
        }

        if snapshot.targets.targets.len() != before_count {
            paths.save_targets(&snapshot.targets).map_err(map_io)?;
            let op_id = record_v3_operation(
                &paths,
                "migrate.v2-to-v3.apply",
                json!({
                    "request_id": request_id,
                    "candidate_targets": plan.candidate_targets.len()
                }),
                json!({
                    "created_targets": created,
                    "skipped_targets": skipped
                }),
            )
            .map_err(map_io)?;
            meta.op_id = Some(op_id);
        }

        meta.warnings.extend(plan.warnings.clone());

        Ok((
            json!({
                "migration": plan.as_json("apply"),
                "created_targets": created,
                "skipped_targets": skipped,
                "noop": meta.op_id.is_none()
            }),
            meta,
        ))
    }

    fn build_v2_to_v3_migration_plan(&self) -> Result<V2ToV3MigrationPlan> {
        let legacy = self.ctx.load_targets()?;
        let existing_v3 = V3StatePaths::from_root(&self.ctx.root).maybe_load_snapshot()?;
        let mut candidate_paths = BTreeMap::<(AgentKind, PathBuf), BTreeSet<String>>::new();
        let mut unresolved = Vec::new();

        for (skill_id, config) in &legacy.skills {
            collect_legacy_target_candidate(
                skill_id,
                AgentKind::Claude,
                config.claude_path.as_deref(),
                &mut candidate_paths,
                &mut unresolved,
            );
            collect_legacy_target_candidate(
                skill_id,
                AgentKind::Codex,
                config.codex_path.as_deref(),
                &mut candidate_paths,
                &mut unresolved,
            );
        }

        let mut synthetic_targets = existing_v3
            .as_ref()
            .map(|snapshot| snapshot.targets.clone())
            .unwrap_or_else(|| V3TargetsFile {
                schema_version: 3,
                targets: Vec::new(),
            });

        let mut candidate_targets = Vec::new();
        for ((agent, path), source_skills) in candidate_paths {
            let existing_target_id = synthetic_targets
                .targets
                .iter()
                .find(|target| {
                    target.agent == agent_kind_as_str(agent)
                        && Path::new(&target.path) == path.as_path()
                })
                .map(|target| target.target_id.clone());

            let target_id = existing_target_id.clone().unwrap_or_else(|| {
                let next_id = unique_target_id_for(
                    agent,
                    path.to_string_lossy().as_ref(),
                    &synthetic_targets,
                );
                synthetic_targets.targets.push(V3ProjectionTarget {
                    target_id: next_id.clone(),
                    agent: agent_kind_as_str(agent).to_string(),
                    path: path.display().to_string(),
                    ownership: target_ownership_as_str(TargetOwnership::Observed).to_string(),
                    capabilities: target_capabilities(TargetOwnership::Observed),
                    created_at: None,
                });
                next_id
            });

            candidate_targets.push(V2ToV3CandidateTarget {
                target_id,
                agent,
                path: path.display().to_string(),
                ownership: TargetOwnership::Observed,
                action: if existing_target_id.is_some() {
                    "skip_existing".to_string()
                } else {
                    "create".to_string()
                },
                existing_target_id,
                source_skills: source_skills.into_iter().collect(),
            });
        }

        let mut warnings = Vec::new();
        if legacy.skills.is_empty() {
            warnings.push("no legacy v2 targets.json entries found".to_string());
        }
        if candidate_targets.is_empty() && unresolved.is_empty() && !legacy.skills.is_empty() {
            warnings
                .push("legacy targets did not yield any migratable target directories".to_string());
        }
        if existing_v3.is_some() {
            warnings.push(
                "existing v3 state detected; migration will merge missing observed targets only"
                    .to_string(),
            );
        }

        Ok(V2ToV3MigrationPlan {
            legacy_skill_count: legacy.skills.len(),
            candidate_targets,
            unresolved,
            warnings,
        })
    }

    fn unsupported_v1_command(
        &self,
        legacy: &'static str,
        replacement: &'static str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let mut failure = CommandFailure::new(
            ErrorCode::UnsupportedV1Command,
            format!(
                "command '{}' was removed in v2, use '{}'",
                legacy, replacement
            ),
        );
        failure.details = json!({
            "removed_command": legacy,
            "replacement": replacement
        });
        Err(failure)
    }
}

#[derive(Debug, Clone)]
struct V2ToV3MigrationPlan {
    legacy_skill_count: usize,
    candidate_targets: Vec<V2ToV3CandidateTarget>,
    unresolved: Vec<V2ToV3Unresolved>,
    warnings: Vec<String>,
}

impl V2ToV3MigrationPlan {
    fn as_json(&self, mode: &str) -> serde_json::Value {
        json!({
            "mode": mode,
            "legacy_skill_count": self.legacy_skill_count,
            "candidate_targets": self.candidate_targets,
            "candidate_bindings": [],
            "unresolved": self.unresolved,
            "warnings": self.warnings,
            "next_steps": [
                "review observed targets",
                "create workspace bindings manually",
                "project skills explicitly after migration"
            ]
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct V2ToV3CandidateTarget {
    target_id: String,
    agent: AgentKind,
    path: String,
    ownership: TargetOwnership,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    existing_target_id: Option<String>,
    source_skills: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct V2ToV3Unresolved {
    skill_id: String,
    agent: AgentKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    reason: String,
    details: serde_json::Value,
}

fn resolve_init_args(args: &InitArgs) -> Result<InitResolved> {
    if args.wizard {
        return run_init_wizard(args);
    }
    Ok(InitResolved {
        from_agent: args.from_agent,
        target: args.target,
        copy: args.copy,
        force: args.force,
        skip_backup: args.skip_backup,
        backup_dir: args.backup_dir.clone(),
    })
}

fn collect_legacy_target_candidate(
    skill_id: &str,
    agent: AgentKind,
    raw_path: Option<&str>,
    out: &mut BTreeMap<(AgentKind, PathBuf), BTreeSet<String>>,
    unresolved: &mut Vec<V2ToV3Unresolved>,
) {
    let Some(raw_path) = raw_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let legacy_path = PathBuf::from(raw_path);
    if !legacy_path.is_absolute() {
        unresolved.push(V2ToV3Unresolved {
            skill_id: skill_id.to_string(),
            agent,
            path: Some(raw_path.to_string()),
            reason: "relative_path".to_string(),
            details: json!({
                "message": "legacy target path must be absolute to migrate safely"
            }),
        });
        return;
    }

    let Some(parent) = legacy_path.parent() else {
        unresolved.push(V2ToV3Unresolved {
            skill_id: skill_id.to_string(),
            agent,
            path: Some(raw_path.to_string()),
            reason: "missing_parent".to_string(),
            details: json!({
                "message": "legacy target path has no parent directory"
            }),
        });
        return;
    };

    if !parent.exists() {
        unresolved.push(V2ToV3Unresolved {
            skill_id: skill_id.to_string(),
            agent,
            path: Some(parent.display().to_string()),
            reason: "target_directory_missing".to_string(),
            details: json!({
                "legacy_path": raw_path,
                "message": "observed target directory does not exist"
            }),
        });
        return;
    }

    out.entry((agent, parent.to_path_buf()))
        .or_default()
        .insert(skill_id.to_string());
}

fn run_init_wizard(args: &InitArgs) -> Result<InitResolved> {
    println!("Loom init wizard");
    println!("Flow: backup -> import -> symlink");

    let from_agent = prompt_target("Import source agent [both/claude/codex]", args.from_agent)?;
    let target = prompt_target("Link target agent [both/claude/codex]", args.target)?;
    let copy = prompt_bool(
        "Use copy mode instead of symlink? (default: no, symlink-first)",
        args.copy,
    )?;
    let force = prompt_bool(
        "Overwrite existing skills with same name? (default: no)",
        args.force,
    )?;
    let skip_backup = prompt_bool(
        "Skip backup before import? (default: no, recommended keep backup)",
        args.skip_backup,
    )?;
    let backup_dir = if skip_backup {
        None
    } else {
        prompt_optional(
            "Backup directory (empty = default state/backups)",
            args.backup_dir.clone(),
        )?
    };

    println!(
        "Selected: from_agent={}, target={}, copy={}, force={}, skip_backup={}, backup_dir={}",
        target_as_str(from_agent),
        target_as_str(target),
        copy,
        force,
        skip_backup,
        backup_dir.as_deref().unwrap_or("default(state/backups)")
    );

    let proceed = prompt_bool("Proceed with init?", true)?;
    if !proceed {
        return Err(anyhow!("init canceled by user"));
    }

    Ok(InitResolved {
        from_agent,
        target,
        copy,
        force,
        skip_backup,
        backup_dir,
    })
}

fn target_as_str(target: Target) -> &'static str {
    match target {
        Target::Claude => "claude",
        Target::Codex => "codex",
        Target::Both => "both",
    }
}

fn prompt_target(message: &str, default: Target) -> Result<Target> {
    loop {
        let input = prompt(message, Some(target_as_str(default)))?;
        if input.is_empty() {
            return Ok(default);
        }
        let value = input.to_lowercase();
        match value.as_str() {
            "both" | "b" | "1" => return Ok(Target::Both),
            "claude" | "c" | "2" => return Ok(Target::Claude),
            "codex" | "x" | "3" => return Ok(Target::Codex),
            _ => {
                eprintln!("invalid value: {} (allowed: both/claude/codex)", input);
            }
        }
    }
}

fn prompt_bool(message: &str, default: bool) -> Result<bool> {
    loop {
        let default_str = if default { "Y/n" } else { "y/N" };
        let input = prompt(message, Some(default_str))?;
        if input.is_empty() {
            return Ok(default);
        }
        let value = input.to_lowercase();
        match value.as_str() {
            "y" | "yes" | "true" | "1" => return Ok(true),
            "n" | "no" | "false" | "0" => return Ok(false),
            _ => {
                eprintln!("invalid value: {} (allowed: y/n)", input);
            }
        }
    }
}

fn prompt_optional(message: &str, default: Option<String>) -> Result<Option<String>> {
    let default_hint = default.as_deref();
    let input = prompt(message, default_hint)?;
    if input.is_empty() {
        return Ok(default);
    }
    Ok(Some(input))
}

fn prompt(message: &str, default: Option<&str>) -> Result<String> {
    match default {
        Some(v) => print!("{} [{}]: ", message, v),
        None => print!("{}: ", message),
    }
    io::stdout().flush().context("failed to flush prompt")?;

    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("failed to read user input")?;
    Ok(line.trim().to_string())
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

fn resolve_targets(target: Target) -> Result<Vec<(&'static str, PathBuf)>> {
    let dirs = resolve_agent_skill_dirs();

    let out = match target {
        Target::Claude => vec![("claude", dirs.claude)],
        Target::Codex => vec![("codex", dirs.codex)],
        Target::Both => vec![("claude", dirs.claude), ("codex", dirs.codex)],
    };
    Ok(out)
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Workspace { command } => match command {
            WorkspaceCommand::Init(_) => "workspace.init",
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
            SkillCommand::Import(_) => "skill.import",
            SkillCommand::Project(_) => "skill.project",
            SkillCommand::Capture(_) => "skill.capture",
            SkillCommand::Link(_) => "skill.link",
            SkillCommand::Use(_) => "skill.use",
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
        Command::Migrate { command } => match command {
            MigrateCommand::V2ToV3(args) => {
                if args.apply {
                    "migrate.v2-to-v3.apply"
                } else {
                    "migrate.v2-to-v3.plan"
                }
            }
        },
        Command::Panel(_) => "panel",
        Command::LegacyInit(_) => "init",
        Command::LegacyAdd(_) => "add",
        Command::LegacyImport(_) => "import",
        Command::LegacyLink(_) => "link",
        Command::LegacyUse(_) => "use",
        Command::LegacySave(_) => "save",
        Command::LegacySnapshot(_) => "snapshot",
        Command::LegacyRelease(_) => "release",
        Command::LegacyRollback(_) => "rollback",
        Command::LegacyDiff(_) => "diff",
        Command::LegacyStatus => "status",
        Command::LegacyDoctor => "doctor",
        Command::LegacyRemote { .. } => "remote",
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
    let leaf = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(agent_kind_as_str(agent));
    let base = format!("target_{}_{}", agent_kind_as_str(agent), slugify(leaf));
    unique_id(
        &base,
        targets
            .targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect(),
    )
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
        if let Some(skill) = args.skill.as_deref() {
            if projection.skill_id != skill {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "instance '{}' belongs to skill '{}' not '{}'",
                        instance_id, projection.skill_id, skill
                    ),
                ));
            }
        }
        if let Some(binding_id) = args.binding.as_deref() {
            if projection.binding_id != binding_id {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "instance '{}' belongs to binding '{}' not '{}'",
                        instance_id, projection.binding_id, binding_id
                    ),
                ));
            }
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

#[derive(Debug, Clone)]
struct ImportCandidate {
    skill: String,
    source: PathBuf,
    origin: String,
}

fn link_skill(
    ctx: &AppContext,
    args: &LinkArgs,
) -> std::result::Result<(Vec<serde_json::Value>, Vec<String>), CommandFailure> {
    let skill_src = ctx.skill_path(&args.skill);
    if !skill_src.exists() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", args.skill),
        ));
    }

    let targets = resolve_targets(args.target).map_err(map_arg)?;
    let mut linked = Vec::new();
    let mut warnings = Vec::new();
    let mut method_used = if args.copy { "copy" } else { "symlink" }.to_string();

    for (name, base) in targets {
        fs::create_dir_all(&base).map_err(map_io)?;
        let dst = base.join(&args.skill);
        remove_path_if_exists(&dst).map_err(map_io)?;

        if args.copy {
            copy_dir_recursive(&skill_src, &dst).map_err(map_io)?;
        } else if let Err(e) = create_symlink_dir(&skill_src, &dst) {
            copy_dir_recursive(&skill_src, &dst).map_err(map_io)?;
            warnings.push(format!(
                "symlink failed for {} ({}), fallback to copy",
                name, e
            ));
            method_used = "copy".to_string();
        }

        linked.push(json!({"target": name, "path": dst}));
    }

    let mut state = ctx.load_targets().map_err(map_io)?;
    let mut config = SkillTargetConfig {
        method: method_used,
        claude_path: None,
        codex_path: None,
    };
    for item in &linked {
        let target = item["target"].as_str().unwrap_or_default();
        let path = item["path"].as_str().unwrap_or_default().to_string();
        match target {
            "claude" => config.claude_path = Some(path),
            "codex" => config.codex_path = Some(path),
            _ => {}
        }
    }
    state.skills.insert(args.skill.clone(), config);
    ctx.save_targets(&state).map_err(map_io)?;

    Ok((linked, warnings))
}

fn collect_import_candidates(args: &ImportArgs) -> Result<(Vec<ImportCandidate>, Vec<String>)> {
    if args.source.is_some() == args.from_agent.is_some() {
        return Err(anyhow!(
            "use exactly one source mode: --source <dir> or --from-agent <claude|codex|both>"
        ));
    }
    if let Some(skill) = &args.skill {
        validate_skill_name(skill)?;
    }

    let mut warnings = Vec::new();
    let mut raw_candidates = Vec::new();

    if let Some(source) = &args.source {
        let source_path = PathBuf::from(source);
        if !source_path.exists() {
            return Err(anyhow!("source does not exist: {}", source_path.display()));
        }

        let has_skill_file = source_path.join("SKILL.md").exists();
        if has_skill_file {
            let skill_name = args.skill.clone().unwrap_or_else(|| {
                source_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            });
            validate_skill_name(&skill_name)?;
            raw_candidates.push(ImportCandidate {
                skill: skill_name,
                source: source_path.clone(),
                origin: format!("source:{}", source_path.display()),
            });
        } else {
            for skill_dir in discover_skill_dirs(&source_path)? {
                let skill_name = skill_dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Some(filter) = &args.skill {
                    if &skill_name != filter {
                        continue;
                    }
                }
                if let Err(err) = validate_skill_name(&skill_name) {
                    warnings.push(format!(
                        "skipping invalid skill directory {}: {}",
                        skill_dir.display(),
                        err
                    ));
                    continue;
                }
                raw_candidates.push(ImportCandidate {
                    skill: skill_name,
                    source: skill_dir,
                    origin: format!("source:{}", source_path.display()),
                });
            }
        }
    } else if let Some(from_agent) = args.from_agent {
        for (agent_name, base) in resolve_targets(from_agent)? {
            if !base.exists() {
                warnings.push(format!(
                    "agent skills directory does not exist: {} ({})",
                    agent_name,
                    base.display()
                ));
                continue;
            }

            for skill_dir in discover_skill_dirs(&base)? {
                let skill_name = skill_dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Some(filter) = &args.skill {
                    if &skill_name != filter {
                        continue;
                    }
                }
                if let Err(err) = validate_skill_name(&skill_name) {
                    warnings.push(format!(
                        "skipping invalid agent skill directory {}: {}",
                        skill_dir.display(),
                        err
                    ));
                    continue;
                }
                raw_candidates.push(ImportCandidate {
                    skill: skill_name,
                    source: skill_dir,
                    origin: format!("agent:{}:{}", agent_name, base.display()),
                });
            }
        }
    }

    let mut dedup = std::collections::BTreeMap::<String, ImportCandidate>::new();
    for candidate in raw_candidates {
        if dedup.contains_key(&candidate.skill) {
            warnings.push(format!(
                "duplicate skill '{}' detected; keeping first candidate",
                candidate.skill
            ));
            continue;
        }
        dedup.insert(candidate.skill.clone(), candidate);
    }

    Ok((dedup.into_values().collect(), warnings))
}

fn backup_agent_skills(
    ctx: &AppContext,
    from_agent: Target,
    backup_dir: Option<&str>,
) -> Result<(serde_json::Value, Vec<String>)> {
    let backup_root = match backup_dir {
        Some(path) => PathBuf::from(path),
        None => ctx.state_dir.join("backups"),
    };
    fs::create_dir_all(&backup_root)
        .with_context(|| format!("failed to create backup root {}", backup_root.display()))?;

    let ts = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let destination = backup_root.join(ts);
    fs::create_dir_all(&destination).with_context(|| {
        format!(
            "failed to create backup destination {}",
            destination.display()
        )
    })?;

    let mut warnings = Vec::new();
    let mut sources = Vec::new();
    let mut total_skills = 0usize;

    for (agent_name, src) in resolve_targets(from_agent)? {
        if !src.exists() {
            warnings.push(format!(
                "backup skipped: {} source not found ({})",
                agent_name,
                src.display()
            ));
            continue;
        }

        let dst = destination.join(format!("{}_skills", agent_name));
        copy_dir_recursive(&src, &dst)?;
        let count = count_skill_dirs(&src)?;
        total_skills += count;
        sources.push(json!({
            "agent": agent_name,
            "source": src,
            "backup": dst,
            "skill_dirs": count
        }));
    }

    let manifest_path = destination.join("backup_manifest.txt");
    let mut manifest = String::new();
    manifest.push_str(&format!(
        "backup_time={}\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    ));
    manifest.push_str(&format!("destination={}\n", destination.display()));
    manifest.push_str("sources=\n");
    for source in &sources {
        let line = format!(
            "- {} ({})\n",
            source["source"].as_str().unwrap_or_default(),
            source["agent"].as_str().unwrap_or_default()
        );
        manifest.push_str(&line);
    }
    manifest.push_str("counts=\n");
    for source in &sources {
        let line = format!(
            "{}_dirs={}\n",
            source["agent"].as_str().unwrap_or_default(),
            source["skill_dirs"].as_u64().unwrap_or(0)
        );
        manifest.push_str(&line);
    }
    manifest.push_str(&format!("total_skill_dirs={}\n", total_skills));
    fs::write(&manifest_path, manifest)
        .with_context(|| format!("failed to write {}", manifest_path.display()))?;

    Ok((
        json!({
            "destination": destination,
            "manifest": manifest_path,
            "total_skill_dirs": total_skills,
            "sources": sources
        }),
        warnings,
    ))
}

fn count_skill_dirs(root: &Path) -> Result<usize> {
    let mut count = 0usize;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            count += 1;
        }
    }
    Ok(count)
}

fn discover_skill_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.is_dir() {
        return Err(anyhow!(
            "import source must be a directory: {}",
            root.display()
        ));
    }

    let mut out = std::collections::BTreeSet::new();
    for entry in WalkDir::new(root).follow_links(true).into_iter() {
        let entry = entry.with_context(|| format!("failed to walk {}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() == "SKILL.md" {
            if let Some(parent) = entry.path().parent() {
                out.insert(parent.to_path_buf());
            }
        }
    }

    Ok(out.into_iter().collect())
}

pub fn list_skills(ctx: &AppContext) -> Result<Vec<String>> {
    if !ctx.skills_dir.exists() {
        return Ok(Vec::new());
    }
    let mut skills = Vec::new();
    for entry in fs::read_dir(&ctx.skills_dir).context("failed to read skills dir")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            skills.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    skills.sort();
    Ok(skills)
}

pub fn remote_status_payload(
    ctx: &AppContext,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let pending_report = ctx.read_pending_report().map_err(map_io)?;
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
    CommandFailure::new(ErrorCode::LockBusy, err.to_string())
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
