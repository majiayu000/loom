use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::SkillInstallArgs;
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::gitops;
use crate::next_action_trace::observe_next_actions;
use crate::state::AppContext;
use crate::state_model::{RegistryStatePaths, RegistryTrustRecord};
use crate::types::ErrorCode;

use super::super::file_ops::{copy_dir_recursive_without_symlinks, rollback_added_skill};
use super::super::helpers::{map_git, map_io, map_lock, map_registry_state};
use super::super::projections::{
    RegistryAuditStateBackup, maybe_autosync_or_queue, record_registry_operation,
    restore_registry_audit_state, snapshot_registry_audit_state,
};
use super::super::provenance::{
    AddSourceResolution, SourceDescriptor, clone_git_source, provenance_record_for_skill,
    save_record_and_lock,
};
use super::super::skill_safety::upsert_trust_record;
use super::super::{App, CommandFailure};
use super::locator::{fetch_plan, local_preview, local_source_descriptor, pin_policy};
use super::{LocatorSource, ParsedLocator, ProviderKind};

const SOURCES_REL: &str = "state/registry/sources.json";
const LOCK_REL: &str = "loom.lock";
const TRUST_REL: &str = "state/registry/trust.json";

struct InstallBackups {
    sources: FileBackup,
    lock: FileBackup,
    trust: FileBackup,
    audit: RegistryAuditStateBackup,
}

struct FileBackup {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

struct AppliedInstall {
    provenance: serde_json::Value,
    trust: RegistryTrustRecord,
    commit: Option<String>,
    op_id: String,
}

struct InstallMutation<'a> {
    ctx: &'a AppContext,
    paths: &'a RegistryStatePaths,
    args: &'a SkillInstallArgs,
    locator: &'a ParsedLocator,
    trust: &'a str,
    request_id: &'a str,
    source: AddSourceResolution,
    dst: &'a Path,
    staging_root: &'a Path,
}

impl App {
    pub(super) fn cmd_provider_install_apply(
        &self,
        args: &SkillInstallArgs,
        locator: ParsedLocator,
        trust: &str,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
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
            .join(format!("tmp-provider-install-{}", Uuid::new_v4()));
        remove_path_if_exists(&staging_root).map_err(map_io)?;
        fs::create_dir_all(&staging_root).map_err(map_io)?;

        let source = match provider_install_source(&self.ctx, &locator, &staging_root, &args.name) {
            Ok(source) => source,
            Err(err) => {
                cleanup_staging(&staging_root);
                return Err(err);
            }
        };
        let backups = InstallBackups::capture(&self.ctx, &paths)?;
        let applied = match apply_install_mutation(InstallMutation {
            ctx: &self.ctx,
            paths: &paths,
            args,
            locator: &locator,
            trust,
            request_id,
            source,
            dst: &dst,
            staging_root: &staging_root,
        }) {
            Ok(applied) => applied,
            Err(err) => {
                cleanup_staging(&staging_root);
                let rollback_errors = backups.rollback(&self.ctx, &paths, &args.name, &dst);
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };
        cleanup_staging(&staging_root);

        let mut meta = Meta {
            op_id: Some(applied.op_id.clone()),
            ..Meta::default()
        };
        if let Some(commit) = &applied.commit {
            maybe_autosync_or_queue(
                &self.ctx,
                "skill.install",
                request_id,
                json!({"skill": args.name, "provider": locator.provider_id(), "commit": commit}),
                &mut meta,
            )?;
        }

        Ok((
            json!({
                "dry_run": false,
                "skill": args.name,
                "path": format!("skills/{}", args.name),
                "resolved_locator": locator.source_json(),
                "pin_policy": pin_policy(&locator, args.policy_profile.as_deref()),
                "staging": {"mode": "isolated", "fetch_plan": fetch_plan(&locator)},
                "provenance": applied.provenance,
                "trust": applied.trust,
                "commit": applied.commit,
                "next_actions": observe_next_actions(
                    "provider.install.applied",
                    [
                        format!("loom skill provenance verify {}", args.name),
                        format!("loom skill scan {}", args.name),
                        format!(
                            "loom skill activate {} --agent <agent> --dry-run",
                            args.name
                        ),
                    ],
                ),
            }),
            meta,
        ))
    }
}

fn apply_install_mutation(
    mutation: InstallMutation<'_>,
) -> std::result::Result<AppliedInstall, CommandFailure> {
    let staging_skill = mutation
        .staging_root
        .join("materialized")
        .join(&mutation.args.name);
    if let Some(parent) = staging_skill.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    copy_dir_recursive_without_symlinks(&mutation.source.copy_source, &staging_skill)
        .map_err(map_io)?;
    if let Some(parent) = mutation.dst.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    fs::rename(&staging_skill, mutation.dst).map_err(map_io)?;

    let record = provenance_record_for_skill(
        &mutation.args.name,
        mutation.source.descriptor,
        mutation.dst,
    )?;
    save_record_and_lock(mutation.ctx, record.clone())?;
    let trust_record = write_install_trust(
        mutation.paths,
        &mutation.args.name,
        mutation.trust,
        mutation.args.review_evidence.as_deref(),
    )?;
    let op_id = record_registry_operation(
        mutation.paths,
        "skill.install",
        json!({
            "skill_id": mutation.args.name,
            "locator": mutation.args.locator,
            "provider_id": mutation.locator.provider_id(),
            "trust": mutation.trust,
            "request_id": mutation.request_id
        }),
        json!({
            "skill_path": format!("skills/{}", mutation.args.name),
            "provenance_path": SOURCES_REL,
            "lock_path": LOCK_REL,
            "trust_path": TRUST_REL,
            "provider_id": mutation.locator.provider_id()
        }),
    )
    .map_err(map_registry_state)?;
    let commit = gitops::commit_paths_if_changed(
        mutation.ctx,
        &[
            &format!("skills/{}", mutation.args.name),
            SOURCES_REL,
            LOCK_REL,
            TRUST_REL,
            "state/registry",
            "state/v3",
            ".gitignore",
        ],
        &format!("install({}): import provider skill", mutation.args.name),
    )
    .map_err(map_git)?;

    Ok(AppliedInstall {
        provenance: json!(record),
        trust: trust_record,
        commit,
        op_id,
    })
}

fn provider_install_source(
    ctx: &AppContext,
    locator: &ParsedLocator,
    staging_root: &Path,
    skill: &str,
) -> std::result::Result<AddSourceResolution, CommandFailure> {
    match locator.provider_kind() {
        ProviderKind::Local => {
            let copy_source = locator.source_path().ok_or_else(|| {
                CommandFailure::new(ErrorCode::InternalError, "missing local source path")
            })?;
            validate_provider_source(&copy_source, locator, skill)?;
            Ok(AddSourceResolution {
                copy_source,
                descriptor: local_source_descriptor(locator)?,
            })
        }
        ProviderKind::Github => {
            let source = github_install_source(ctx, locator, staging_root)?;
            validate_provider_source(&source.copy_source, locator, skill)?;
            Ok(source)
        }
    }
}

fn github_install_source(
    ctx: &AppContext,
    locator: &ParsedLocator,
    staging_root: &Path,
) -> std::result::Result<AddSourceResolution, CommandFailure> {
    let LocatorSource::Github {
        repository,
        clone_url,
    } = &locator.source
    else {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            "expected github locator",
        ));
    };
    clone_git_source(
        ctx,
        clone_url,
        locator.requested_ref.as_deref(),
        locator.subdir.clone(),
        staging_root,
        |commit, tree| SourceDescriptor {
            provider: locator.provider_id().to_string(),
            locator: locator.raw.clone(),
            repository: Some(repository.clone()),
            path: None,
            subdir: locator.subdir.clone(),
            requested_ref: locator.requested_ref.clone(),
            resolved_commit: Some(commit),
            tree_sha: Some(tree),
        },
    )
}

fn validate_provider_source(
    path: &Path,
    locator: &ParsedLocator,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    let preview = local_preview(path, Some(skill))?;
    let digest = preview["provenance"]["digest"]
        .as_str()
        .ok_or_else(|| CommandFailure::new(ErrorCode::InternalError, "missing preview digest"))?;
    if matches!(locator.provider_kind(), ProviderKind::Local)
        && locator.requested_ref.as_deref() != Some(digest)
    {
        let mut failure = CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "local provider digest pin does not match source content",
        );
        failure.details = json!({"requested_ref": locator.requested_ref, "actual_digest": digest});
        return Err(failure);
    }
    if preview["safety"]["summary"]["critical"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        let mut failure = CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "critical safety findings block provider install",
        );
        failure.details = json!({"safety": preview["safety"]});
        return Err(failure);
    }
    Ok(())
}

fn write_install_trust(
    paths: &RegistryStatePaths,
    skill: &str,
    trust: &str,
    review_evidence: Option<&str>,
) -> std::result::Result<RegistryTrustRecord, CommandFailure> {
    let mut trust_file = paths.load_trust().map_err(map_registry_state)?;
    let reason = review_evidence
        .map(|evidence| format!("provider install reviewed evidence: {evidence}"))
        .or_else(|| Some("provider install default trust".to_string()));
    let record = upsert_trust_record(&mut trust_file, skill, trust, false, reason);
    paths.save_trust(&trust_file).map_err(map_registry_state)?;
    Ok(record)
}

impl InstallBackups {
    fn capture(
        ctx: &AppContext,
        paths: &RegistryStatePaths,
    ) -> std::result::Result<Self, CommandFailure> {
        Ok(Self {
            sources: FileBackup::capture(ctx.root.join(SOURCES_REL))?,
            lock: FileBackup::capture(ctx.root.join(LOCK_REL))?,
            trust: FileBackup::capture(ctx.root.join(TRUST_REL))?,
            audit: snapshot_registry_audit_state(paths).map_err(map_registry_state)?,
        })
    }

    fn rollback(
        &self,
        ctx: &AppContext,
        paths: &RegistryStatePaths,
        skill: &str,
        dst: &Path,
    ) -> Vec<Value> {
        let mut errors = Vec::new();
        rollback_added_skill(ctx, &format!("skills/{skill}"), dst);
        self.sources.restore(&mut errors);
        self.lock.restore(&mut errors);
        self.trust.restore(&mut errors);
        if let Err(err) = restore_registry_audit_state(paths, &self.audit) {
            errors.push(json!({"step": "restore_registry_audit_state", "error": err.to_string()}));
        }
        let _ = gitops::run_git_allow_failure(
            ctx,
            &[
                "reset",
                "HEAD",
                "--",
                &format!("skills/{skill}"),
                SOURCES_REL,
                LOCK_REL,
                TRUST_REL,
                "state/registry",
            ],
        );
        errors
    }
}

impl FileBackup {
    fn capture(path: PathBuf) -> std::result::Result<Self, CommandFailure> {
        let contents = match fs::read(&path) {
            Ok(contents) => Some(contents),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(map_io(err)),
        };
        Ok(Self { path, contents })
    }

    fn restore(&self, errors: &mut Vec<Value>) {
        let result = if let Some(contents) = &self.contents {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent).and_then(|_| fs::write(&self.path, contents))
            } else {
                fs::write(&self.path, contents)
            }
        } else {
            remove_path_if_exists(&self.path)
        };
        if let Err(err) = result {
            errors.push(json!({"step": "restore_file", "path": self.path.display().to_string(), "error": err.to_string()}));
        }
    }
}

fn cleanup_staging(path: &Path) {
    let _ = remove_path_if_exists(path);
}
