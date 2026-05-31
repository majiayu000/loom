use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tar::{Archive, Builder, EntryType, Header};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::cli::{
    BackupCommand, BackupExportArgs, BackupFormat, BackupInspectArgs, BackupRestoreArgs,
};
use crate::gitops;
use crate::state::{AppContext, remove_path_if_exists};
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::file_ops::copy_dir_recursive_preserving_symlinks;
use super::helpers::{map_git, map_io, map_registry_state};
use super::{App, CommandFailure};

const BACKUP_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifest {
    schema_version: u32,
    created_at: DateTime<Utc>,
    loom_version: String,
    source_root: String,
    head: String,
    counts: BackupCounts,
    included_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupCounts {
    skills: usize,
    trash_entries: usize,
    operations: usize,
}

struct InspectedBackup {
    _temp: TempPath,
    root_dir: PathBuf,
    manifest: BackupManifest,
    bundle_path: PathBuf,
    bundle_verify_output: String,
}

struct TempPath {
    path: PathBuf,
    keep: bool,
}

impl TempPath {
    fn new(prefix: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self { path, keep: false })
    }

    fn new_in(parent: &Path, prefix: &str) -> Result<Self> {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        let path = parent.join(format!("{prefix}-{}", Uuid::new_v4().simple()));
        fs::create_dir(&path).with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self { path, keep: false })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn take(mut self) -> PathBuf {
        self.keep = true;
        self.path.clone()
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl App {
    pub fn cmd_backup(
        &self,
        command: &BackupCommand,
    ) -> std::result::Result<(serde_json::Value, crate::envelope::Meta), CommandFailure> {
        match command {
            BackupCommand::Export(args) => self.cmd_backup_export(args),
            BackupCommand::Inspect(args) => self.cmd_backup_inspect(args),
            BackupCommand::Restore(args) => self.cmd_backup_restore(args),
        }
    }

    fn cmd_backup_export(
        &self,
        args: &BackupExportArgs,
    ) -> std::result::Result<(serde_json::Value, crate::envelope::Meta), CommandFailure> {
        if args.format != BackupFormat::Tar {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "only --format tar is supported",
            ));
        }

        let snapshot = self.require_backup_source_ready()?;
        let created_at = Utc::now();
        let stamp = created_at.format("%Y%m%dT%H%M%SZ").to_string();
        let artifact_root = format!("loom-backup-{stamp}");
        let output = args.output.clone().unwrap_or_else(|| {
            self.ctx
                .root
                .join("backups")
                .join(format!("{artifact_root}.tar"))
        });

        let manifest = BackupManifest {
            schema_version: BACKUP_SCHEMA_VERSION,
            created_at,
            loom_version: env!("CARGO_PKG_VERSION").to_string(),
            source_root: self.ctx.root.display().to_string(),
            head: gitops::head(&self.ctx).map_err(map_git)?,
            counts: BackupCounts {
                skills: count_immediate_dirs(&self.ctx.skills_dir).map_err(map_io)?,
                trash_entries: count_immediate_entries(&self.ctx.root.join("trash"))
                    .map_err(map_io)?,
                operations: snapshot.operations.len(),
            },
            included_paths: included_paths(&self.ctx, args.include_target_cache),
        };

        export_tar(
            &self.ctx,
            &output,
            &artifact_root,
            &manifest,
            args.include_target_cache,
        )?;
        Ok((
            json!({
                "artifact": output.display().to_string(),
                "format": backup_format_as_str(args.format),
                "manifest": manifest,
                "target_cache_included": args.include_target_cache
                    && self.ctx.root.join("state/target-cache").exists(),
            }),
            crate::envelope::Meta::default(),
        ))
    }

    fn cmd_backup_inspect(
        &self,
        args: &BackupInspectArgs,
    ) -> std::result::Result<(serde_json::Value, crate::envelope::Meta), CommandFailure> {
        let inspected = inspect_backup_artifact(&args.artifact)?;
        Ok((
            json!({
                "artifact": args.artifact.display().to_string(),
                "valid": true,
                "bundle_verified": true,
                "bundle_verify_output": inspected.bundle_verify_output,
                "manifest": inspected.manifest,
            }),
            crate::envelope::Meta::default(),
        ))
    }

    fn cmd_backup_restore(
        &self,
        args: &BackupRestoreArgs,
    ) -> std::result::Result<(serde_json::Value, crate::envelope::Meta), CommandFailure> {
        let inspected = inspect_backup_artifact(&args.artifact)?;
        validate_restore_root(&self.ctx.root, args.force_empty_root)?;

        let parent = parent_or_cwd(&self.ctx.root);
        let staging = TempPath::new_in(parent, ".loom-restore").map_err(map_io)?;
        clone_bundle_to(&inspected.bundle_path, staging.path()).map_err(map_git)?;
        overlay_registry_snapshot(&inspected.root_dir.join("registry"), staging.path())?;
        let restored_head = verify_restored_root(staging.path())?;

        if self.ctx.root.exists() {
            remove_path_if_exists(&self.ctx.root).map_err(map_io)?;
        }
        let staged_path = staging.take();
        fs::rename(&staged_path, &self.ctx.root).map_err(map_io)?;

        Ok((
            json!({
                "restored": true,
                "root": self.ctx.root.display().to_string(),
                "head": restored_head,
                "source_head": inspected.manifest.head,
                "counts": inspected.manifest.counts,
            }),
            crate::envelope::Meta::default(),
        ))
    }

    fn require_backup_source_ready(
        &self,
    ) -> std::result::Result<crate::state_model::RegistrySnapshot, CommandFailure> {
        self.ctx
            .ensure_not_loom_tool_repo_root()
            .map_err(map_arg_error)?;
        if !self.ctx.root.is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("registry root does not exist: {}", self.ctx.root.display()),
            ));
        }
        if !gitops::repo_is_initialized(&self.ctx).map_err(map_git)? {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "registry root is not initialized as a Git repository: {}",
                    self.ctx.root.display()
                ),
            ));
        }
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let snapshot = paths
            .maybe_load_snapshot()
            .map_err(map_registry_state)?
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!(
                        "registry state not initialized under {}",
                        paths.registry_dir.display()
                    ),
                )
            })?;
        gitops::head(&self.ctx).map_err(map_git)?;
        Ok(snapshot)
    }
}

fn export_tar(
    ctx: &AppContext,
    output: &Path,
    artifact_root: &str,
    manifest: &BackupManifest,
    include_target_cache: bool,
) -> std::result::Result<(), CommandFailure> {
    let temp = TempPath::new("loom-backup-export").map_err(map_io)?;
    let bundle_path = temp.path().join("git.bundle");
    create_full_bundle(&ctx.root, &bundle_path).map_err(map_git)?;
    verify_bundle_file(&bundle_path).map_err(map_git)?;

    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(map_io)?;
    let mut builder = Builder::new(file);
    let mtime = manifest.created_at.timestamp().max(0) as u64;

    append_json(
        &mut builder,
        &Path::new(artifact_root).join("manifest.json"),
        manifest,
        mtime,
    )
    .map_err(map_io)?;
    builder
        .append_path_with_name(&bundle_path, Path::new(artifact_root).join("git.bundle"))
        .map_err(map_io)?;
    append_path_or_empty_dir(
        &mut builder,
        &ctx.skills_dir,
        &Path::new(artifact_root).join("registry/skills"),
        mtime,
    )
    .map_err(map_io)?;
    append_path_or_empty_dir(
        &mut builder,
        &ctx.root.join("trash"),
        &Path::new(artifact_root).join("registry/trash"),
        mtime,
    )
    .map_err(map_io)?;
    append_tree(
        &mut builder,
        &ctx.root.join("state/registry"),
        &Path::new(artifact_root).join("registry/state/registry"),
    )
    .map_err(map_io)?;
    builder
        .append_path_with_name(
            ctx.root.join(".gitignore"),
            Path::new(artifact_root).join("registry/.gitignore"),
        )
        .map_err(map_io)?;
    if include_target_cache {
        let target_cache = ctx.root.join("state/target-cache");
        if target_cache.exists() {
            append_tree(
                &mut builder,
                &target_cache,
                &Path::new(artifact_root).join("registry/state/target-cache"),
            )
            .map_err(map_io)?;
        }
    }
    builder.finish().map_err(map_io)?;
    Ok(())
}

fn append_json<T: Serialize>(
    builder: &mut Builder<File>,
    archive_path: &Path,
    value: &T,
    mtime: u64,
) -> Result<()> {
    let mut body = serde_json::to_vec_pretty(value)?;
    body.push(b'\n');
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(0o644);
    header.set_mtime(mtime);
    header.set_size(body.len() as u64);
    header.set_cksum();
    builder.append_data(&mut header, archive_path, Cursor::new(body))?;
    Ok(())
}

fn append_path_or_empty_dir(
    builder: &mut Builder<File>,
    src: &Path,
    archive_path: &Path,
    mtime: u64,
) -> Result<()> {
    if src.exists() {
        append_tree(builder, src, archive_path)
    } else {
        append_empty_dir(builder, archive_path, mtime)
    }
}

fn append_tree(builder: &mut Builder<File>, src: &Path, archive_path: &Path) -> Result<()> {
    if !src.exists() {
        return Err(anyhow!(
            "required backup path does not exist: {}",
            src.display()
        ));
    }
    if src.is_file() {
        builder.append_path_with_name(src, archive_path)?;
        return Ok(());
    }
    for entry in WalkDir::new(src)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
    {
        let entry = entry.with_context(|| format!("failed to walk {}", src.display()))?;
        let rel = entry.path().strip_prefix(src)?;
        let name = if rel.as_os_str().is_empty() {
            archive_path.to_path_buf()
        } else {
            archive_path.join(rel)
        };
        if entry.file_type().is_dir() {
            builder.append_dir(&name, entry.path())?;
        } else {
            builder.append_path_with_name(entry.path(), &name)?;
        }
    }
    Ok(())
}

fn append_empty_dir(builder: &mut Builder<File>, archive_path: &Path, mtime: u64) -> Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Directory);
    header.set_mode(0o755);
    header.set_mtime(mtime);
    header.set_size(0);
    header.set_cksum();
    builder.append_data(&mut header, archive_path, io::empty())?;
    Ok(())
}

fn inspect_backup_artifact(
    artifact: &Path,
) -> std::result::Result<InspectedBackup, CommandFailure> {
    if !artifact.is_file() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("backup artifact does not exist: {}", artifact.display()),
        ));
    }
    let temp = TempPath::new("loom-backup-inspect").map_err(map_io)?;
    let file = File::open(artifact).map_err(map_io)?;
    let mut archive = Archive::new(file);
    for entry in archive.entries().map_err(map_io)? {
        let mut entry = entry.map_err(map_io)?;
        validate_archive_entry(&entry).map_err(map_arg_error)?;
        if !entry.unpack_in(temp.path()).map_err(map_io)? {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "backup artifact contains an unsafe path",
            ));
        }
    }

    let root_dir = find_extracted_root(temp.path())?;
    let manifest_path = root_dir.join("manifest.json");
    let manifest: BackupManifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(map_io)?)
            .map_err(|err| CommandFailure::new(ErrorCode::StateCorrupt, err.to_string()))?;
    if manifest.schema_version != BACKUP_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported backup schema_version {}",
                manifest.schema_version
            ),
        ));
    }
    for required in [
        "git.bundle",
        "registry/skills",
        "registry/trash",
        "registry/state/registry",
        "registry/.gitignore",
    ] {
        if !root_dir.join(required).exists() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("backup artifact missing required path: {required}"),
            ));
        }
    }
    let bundle_path = root_dir.join("git.bundle");
    let bundle_verify_output = verify_bundle_file(&bundle_path).map_err(map_git)?;
    Ok(InspectedBackup {
        _temp: temp,
        root_dir,
        manifest,
        bundle_path,
        bundle_verify_output,
    })
}

fn validate_archive_entry(entry: &tar::Entry<'_, File>) -> Result<()> {
    let path = entry.path()?;
    validate_relative_path(&path)?;
    let entry_type = entry.header().entry_type();
    if entry_type == EntryType::Link {
        return Err(anyhow!("hard links are not supported in backup artifacts"));
    }
    if entry_type == EntryType::Symlink
        && let Some(link) = entry.link_name()?
    {
        validate_relative_path(&link)?;
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(anyhow!("archive path must not be empty"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow!(
                    "archive path is not safely relative: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn find_extracted_root(temp: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    let entries = fs::read_dir(temp)
        .map_err(map_io)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(map_io)?;
    if entries.len() != 1 {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "backup artifact must contain exactly one top-level directory",
        ));
    }
    let root = entries[0].path();
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if !root.is_dir() || !name.starts_with("loom-backup-") {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "backup artifact top-level directory must be named loom-backup-*",
        ));
    }
    Ok(root)
}

fn validate_restore_root(
    root: &Path,
    force_empty_root: bool,
) -> std::result::Result<(), CommandFailure> {
    AppContext::new(Some(root.to_path_buf()))
        .map_err(map_io)?
        .ensure_not_loom_tool_repo_root()
        .map_err(map_arg_error)?;
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(map_io(err)),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "restore root must be an empty directory: {}",
                root.display()
            ),
        ));
    }
    if fs::read_dir(root).map_err(map_io)?.next().is_none() {
        return Ok(());
    }
    if !force_empty_root {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "restore root is not empty: {}; use --force-empty-root only for empty scaffolding",
                root.display()
            ),
        ));
    }
    if !contains_only_safe_scaffolding(root).map_err(map_io)? {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "restore root contains files that cannot be overwritten: {}",
                root.display()
            ),
        ));
    }
    clear_safe_scaffolding(root).map_err(map_io)
}

fn contains_only_safe_scaffolding(root: &Path) -> io::Result<bool> {
    for entry in WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
    {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name == ".DS_Store" || name == ".gitkeep" {
                continue;
            }
        }
        return Ok(false);
    }
    Ok(true)
}

fn clear_safe_scaffolding(root: &Path) -> io::Result<()> {
    for entry in WalkDir::new(root)
        .min_depth(1)
        .contents_first(true)
        .follow_links(false)
        .into_iter()
    {
        let entry = entry?;
        if entry.file_type().is_dir() {
            fs::remove_dir(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn overlay_registry_snapshot(
    registry_snapshot: &Path,
    dst_root: &Path,
) -> std::result::Result<(), CommandFailure> {
    replace_dir(&registry_snapshot.join("skills"), &dst_root.join("skills"))?;
    replace_dir(&registry_snapshot.join("trash"), &dst_root.join("trash"))?;
    replace_dir(
        &registry_snapshot.join("state/registry"),
        &dst_root.join("state/registry"),
    )?;
    remove_path_if_exists(&dst_root.join(".gitignore")).map_err(map_io)?;
    fs::copy(
        registry_snapshot.join(".gitignore"),
        dst_root.join(".gitignore"),
    )
    .map_err(map_io)?;
    Ok(())
}

fn replace_dir(src: &Path, dst: &Path) -> std::result::Result<(), CommandFailure> {
    remove_path_if_exists(dst).map_err(map_io)?;
    copy_dir_recursive_preserving_symlinks(src, dst).map_err(map_io)
}

fn verify_restored_root(root: &Path) -> std::result::Result<String, CommandFailure> {
    for required in [".git", "skills", "state/registry"] {
        if !root.join(required).exists() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("restored root missing required path: {required}"),
            ));
        }
    }
    let ctx = AppContext::new(Some(root.to_path_buf())).map_err(map_io)?;
    if !gitops::repo_is_initialized(&ctx).map_err(map_git)? {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            "restored root is not a Git repository",
        ));
    }
    RegistryStatePaths::from_root(root)
        .load_snapshot()
        .map_err(map_registry_state)?;
    gitops::head(&ctx).map_err(map_git)
}

fn create_full_bundle(root: &Path, bundle_path: &Path) -> Result<()> {
    let mut command = git_base(root);
    command
        .arg("bundle")
        .arg("create")
        .arg(bundle_path)
        .arg("--all");
    run_git_command(command, "git bundle create")?;
    Ok(())
}

fn verify_bundle_file(bundle_path: &Path) -> Result<String> {
    if !bundle_path.is_file() {
        return Err(anyhow!(
            "bundle file does not exist: {}",
            bundle_path.display()
        ));
    }
    let verify_dir = TempPath::new("loom-bundle-verify")?;
    let mut init = git_base(verify_dir.path());
    init.arg("init").arg("-q");
    run_git_command(init, "git init")?;

    let mut verify = git_base(verify_dir.path());
    verify.arg("bundle").arg("verify").arg(bundle_path);
    run_git_command(verify, "git bundle verify")
}

fn clone_bundle_to(bundle_path: &Path, dst: &Path) -> Result<()> {
    let parent = parent_or_cwd(dst);
    let mut command = git_base(parent);
    command.arg("clone").arg(bundle_path).arg(dst);
    run_git_command(command, "git clone backup bundle")?;

    let mut name = git_base(dst);
    name.args(["config", "--local", "user.name", "loom"]);
    run_git_command(name, "git config user.name")?;
    let mut email = git_base(dst);
    email.args(["config", "--local", "user.email", "loom@local"]);
    run_git_command(email, "git config user.email")?;
    Ok(())
}

fn git_base(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .arg("-c")
        .arg("protocol.file.allow=always")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg("protocol.ssh.allow=always");
    command
}

fn run_git_command(mut command: Command, action: &str) -> Result<String> {
    let output = command
        .output()
        .with_context(|| format!("failed to run {action}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("{action} failed: {stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn count_immediate_dirs(path: &Path) -> io::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in fs::read_dir(path)? {
        if entry?.file_type()?.is_dir() {
            count += 1;
        }
    }
    Ok(count)
}

fn count_immediate_entries(path: &Path) -> io::Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    fs::read_dir(path)?.try_fold(0usize, |count, entry| entry.map(|_| count + 1))
}

fn included_paths(ctx: &AppContext, include_target_cache: bool) -> Vec<String> {
    let mut paths = vec![
        "skills/".to_string(),
        "trash/".to_string(),
        "state/registry/".to_string(),
        ".gitignore".to_string(),
    ];
    if include_target_cache && ctx.root.join("state/target-cache").exists() {
        paths.push("state/target-cache/".to_string());
    }
    paths
}

fn backup_format_as_str(format: BackupFormat) -> &'static str {
    match format {
        BackupFormat::Tar => "tar",
    }
}

fn parent_or_cwd(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn map_arg_error(err: anyhow::Error) -> CommandFailure {
    let message = err.to_string();
    let message = message
        .strip_prefix("ARG_INVALID:")
        .map(str::trim)
        .unwrap_or(&message)
        .to_string();
    CommandFailure::new(ErrorCode::ArgInvalid, message)
}
