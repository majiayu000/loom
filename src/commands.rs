use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::cli::{
    AddArgs, Cli, Command, DiffArgs, ImportArgs, InitArgs, LinkArgs, OpsCommand, ReleaseArgs,
    RemoteCommand, RollbackArgs, SaveArgs, SkillCommand, SkillOnlyArgs, SyncCommand, Target,
    WorkspaceCommand,
};
use crate::envelope::{Envelope, Meta};
use crate::gitops;
use crate::state::{AppContext, remove_path_if_exists};
use crate::types::{ErrorCode, SkillTargetConfig, SyncState};

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
        gitops::ensure_repo_initialized(&ctx)?;
        ctx.ensure_gitignore_entries()?;
        ensure_initial_commit(&ctx)?;
        Ok(Self { ctx })
    }

    pub fn execute(&self, cli: Cli) -> Result<(Envelope, i32)> {
        let request_id = cli.request_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let result = match &cli.command {
            Command::Workspace { command } => match command {
                WorkspaceCommand::Init(args) => self.cmd_init(args, &request_id),
                WorkspaceCommand::Status => self.cmd_status(),
                WorkspaceCommand::Doctor => self.cmd_doctor(),
                WorkspaceCommand::Remote { command } => self.cmd_remote(command),
            },
            Command::Skill { command } => match command {
                SkillCommand::Add(args) => self.cmd_add(args, &request_id),
                SkillCommand::Import(args) => self.cmd_import(args, &request_id),
                SkillCommand::Link(args) | SkillCommand::Use(args) => self.cmd_link(args),
                SkillCommand::Save(args) => self.cmd_save(args, &request_id),
                SkillCommand::Snapshot(args) => self.cmd_snapshot(args, &request_id),
                SkillCommand::Release(args) => self.cmd_release(args, &request_id),
                SkillCommand::Rollback(args) => self.cmd_rollback(args, &request_id),
                SkillCommand::Diff(args) => self.cmd_diff(args),
            },
            Command::Sync { command } => self.cmd_sync(command),
            Command::Ops { command } => self.cmd_ops(command),
            Command::Panel(_) => Ok((json!({"message": "panel handled in main"}), Meta::default())),

            Command::LegacyInit(_) => self.unsupported_v1_command("init", "workspace init"),
            Command::LegacyAdd(_) => self.unsupported_v1_command("add", "skill add"),
            Command::LegacyImport(_) => self.unsupported_v1_command("import", "skill import"),
            Command::LegacyLink(_) => self.unsupported_v1_command("link", "skill link"),
            Command::LegacyUse(_) => self.unsupported_v1_command("use", "skill use"),
            Command::LegacySave(_) => self.unsupported_v1_command("save", "skill save"),
            Command::LegacySnapshot(_) => {
                self.unsupported_v1_command("snapshot", "skill snapshot")
            }
            Command::LegacyRelease(_) => self.unsupported_v1_command("release", "skill release"),
            Command::LegacyRollback(_) => {
                self.unsupported_v1_command("rollback", "skill rollback")
            }
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

        let (import_data, mut import_meta) = self.cmd_import(&import_args, request_id)?;

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
            );
        }

        Ok((json!({"skill": args.name, "path": dst}), meta))
    }

    pub fn cmd_import(
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
            );
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

    pub fn cmd_link(
        &self,
        args: &LinkArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let (linked, warnings) = link_skill(&self.ctx, args)?;

        Ok((
            json!({"skill": args.skill, "links": linked}),
            Meta {
                warnings,
                sync_state: None,
            },
        ))
    }

    pub fn cmd_save(
        &self,
        args: &SaveArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
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
        );

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
        ensure_skill_exists(&self.ctx, &args.skill)?;
        let _lock = self.ctx.lock_skill(&args.skill).map_err(map_lock)?;

        let short = gitops::short_head(&self.ctx).map_err(map_git)?;
        let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
        let tag = format!("snapshot/{}/{}-{}", args.skill, ts, short);
        gitops::create_tag(&self.ctx, &tag).map_err(map_git)?;

        let mut meta = Meta::default();
        maybe_autosync_or_queue(
            &self.ctx,
            "snapshot",
            request_id,
            json!({"skill": args.skill, "tag": tag}),
            &mut meta,
        );

        Ok((json!({"skill": args.skill, "tag": tag}), meta))
    }

    pub fn cmd_release(
        &self,
        args: &ReleaseArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
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
        );

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
        );

        Ok((
            json!({"skill": args.skill, "reference": reference, "commit": commit, "noop": false}),
            meta,
        ))
    }

    pub fn cmd_diff(
        &self,
        args: &DiffArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
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
        let head = gitops::head(&self.ctx).ok();
        let branch = gitops::run_git(&self.ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).ok();
        let pending = self.ctx.pending_count().map_err(map_io)?;

        let (remote, meta) = remote_status_payload(&self.ctx)?;
        let status_short = gitops::run_git(&self.ctx, &["status", "--short"]).unwrap_or_default();

        Ok((
            json!({
                "skills": skills,
                "git": {"head": head, "branch": branch, "status_short": status_short},
                "remote": remote,
                "pending_ops": pending
            }),
            meta,
        ))
    }

    pub fn cmd_doctor(&self) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let fsck = gitops::fsck(&self.ctx);
        let fsck_ok = fsck.is_ok();
        let fsck_output = fsck.unwrap_or_else(|e| e.to_string());
        let pending = self.ctx.pending_count().map_err(map_io)?;
        let targets_ok = self.ctx.load_targets().is_ok();

        let healthy = fsck_ok && targets_ok;

        Ok((
            json!({
                "healthy": healthy,
                "checks": {
                    "git_fsck": {"ok": fsck_ok, "output": fsck_output},
                    "targets_file": {"ok": targets_ok},
                    "pending_queue": {"count": pending}
                }
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_remote(
        &self,
        command: &RemoteCommand,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        match command {
            RemoteCommand::Set { url } => {
                gitops::set_remote_origin(&self.ctx, url).map_err(map_git)?;
                Ok((json!({"remote": "origin", "url": url}), Meta::default()))
            }
            RemoteCommand::Status => {
                let (remote, meta) = remote_status_payload(&self.ctx)?;
                Ok((json!({"remote": remote}), meta))
            }
        }
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
                let res = sync_push_internal(&self.ctx)?;
                Ok((json!({"result": res}), Meta::default()))
            }
            SyncCommand::Pull => {
                if !gitops::remote_exists(&self.ctx) {
                    return Err(CommandFailure::new(
                        ErrorCode::ArgInvalid,
                        "remote origin not configured",
                    ));
                }
                gitops::fetch_origin_main(&self.ctx).map_err(map_remote_unreachable)?;
                gitops::pull_rebase_main(&self.ctx).map_err(map_replay_conflict)?;
                let replay = sync_replay_internal(&self.ctx)?;
                Ok((
                    json!({"result": "pulled", "replay": replay}),
                    Meta::default(),
                ))
            }
            SyncCommand::Replay => {
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
                let ops = self.ctx.read_pending().map_err(map_io)?;
                Ok((json!({"count": ops.len(), "ops": ops}), Meta::default()))
            }
            OpsCommand::Retry => {
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
                let purged = self.ctx.pending_count().map_err(map_io)?;
                self.ctx.clear_pending().map_err(map_io)?;
                Ok((json!({"purged": purged}), Meta::default()))
            }
        }
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
    if !ctx.skill_path(skill).exists() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!("source does not exist: {}", src.display()));
    }
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in WalkDir::new(src).follow_links(true).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
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
    let home = std::env::var("HOME").map_err(|_| anyhow!("HOME is not set"))?;
    let claude =
        std::env::var("CLAUDE_SKILLS_DIR").unwrap_or_else(|_| format!("{}/.claude/skills", home));
    let codex =
        std::env::var("CODEX_SKILLS_DIR").unwrap_or_else(|_| format!("{}/.codex/skills", home));

    let out = match target {
        Target::Claude => vec![("claude", PathBuf::from(claude))],
        Target::Codex => vec![("codex", PathBuf::from(codex))],
        Target::Both => vec![
            ("claude", PathBuf::from(claude)),
            ("codex", PathBuf::from(codex)),
        ],
    };
    Ok(out)
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Workspace { command } => match command {
            WorkspaceCommand::Init(_) => "workspace.init",
            WorkspaceCommand::Status => "workspace.status",
            WorkspaceCommand::Doctor => "workspace.doctor",
            WorkspaceCommand::Remote { .. } => "workspace.remote",
        },
        Command::Skill { command } => match command {
            SkillCommand::Add(_) => "skill.add",
            SkillCommand::Import(_) => "skill.import",
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
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
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
    let pending = ctx.pending_count().map_err(map_io)?;

    if !gitops::remote_exists(ctx) {
        return Ok((
            json!({
                "configured": false,
                "pending_ops": pending,
                "sync_state": SyncState::LocalOnly,
            }),
            Meta {
                warnings: vec!["remote origin not configured".to_string()],
                sync_state: Some(SyncState::LocalOnly),
            },
        ));
    }

    let url = gitops::remote_url(ctx)
        .map_err(map_git)?
        .unwrap_or_default();
    let mut meta = Meta::default();

    if let Err(err) = gitops::fetch_origin_main(ctx) {
        meta.warnings.push(format!("remote fetch failed: {}", err));
        meta.sync_state = Some(SyncState::PendingPush);
        return Ok((
            json!({
                "configured": true,
                "remote": "origin",
                "url": url,
                "pending_ops": pending,
                "sync_state": SyncState::PendingPush,
            }),
            meta,
        ));
    }

    let (ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
    let sync_state = if pending > 0 {
        SyncState::PendingPush
    } else if ahead == 0 && behind == 0 {
        SyncState::Synced
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
) {
    if !gitops::remote_exists(ctx) {
        let _ = ctx.append_pending(command, details, request_id.to_string());
        meta.sync_state = Some(SyncState::PendingPush);
        meta.warnings
            .push("remote origin not configured, operation queued".to_string());
        return;
    }

    match sync_push_internal(ctx) {
        Ok(_) => {
            meta.sync_state = Some(SyncState::Synced);
        }
        Err(err) => {
            let _ = ctx.append_pending(command, details, request_id.to_string());
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
}

fn sync_push_internal(ctx: &AppContext) -> std::result::Result<&'static str, CommandFailure> {
    if !gitops::remote_exists(ctx) {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "remote origin not configured",
        ));
    }

    gitops::fetch_origin_main(ctx).map_err(map_remote_unreachable)?;
    let (_ahead, behind) = gitops::ahead_behind_main(ctx).map_err(map_git)?;
    if behind > 0 {
        return Err(CommandFailure::new(
            ErrorCode::RemoteDiverged,
            "local branch is behind origin/main",
        ));
    }
    gitops::push_main_with_tags(ctx).map_err(map_push_rejected)?;
    ctx.clear_pending().map_err(map_io)?;
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
