use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::cli::{AddArgs, SkillOnlyArgs, SkillProvenanceCommand};
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_git, map_io, validate_skill_name};
use super::{App, CommandFailure};

const SOURCES_REL: &str = "state/registry/sources.json";
const LOCK_REL: &str = "loom.lock";

#[derive(Debug, Clone)]
pub(crate) struct AddSourceResolution {
    pub copy_source: PathBuf,
    pub descriptor: SourceDescriptor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SkillSourcesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub sources: Vec<SkillSourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SkillSourceRecord {
    pub skill_id: String,
    pub source: SourceDescriptor,
    pub artifact: ArtifactDescriptor,
    pub imported_at: DateTime<Utc>,
    pub importer_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceDescriptor {
    pub provider: String,
    pub locator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub subdir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArtifactDescriptor {
    pub digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProvenanceDigestStatus {
    pub recorded_digest: String,
    pub current_digest: String,
    pub lock_digest: Option<String>,
    pub lock_present: bool,
    pub matches: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoomLockFile {
    version: u32,
    skills: BTreeMap<String, LoomLockSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoomLockSkill {
    source: String,
    provider: String,
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    requested_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tree_sha: Option<String>,
    digest: String,
    agents: Vec<String>,
    scope: String,
}

impl App {
    pub fn cmd_skill_provenance(
        &self,
        command: &SkillProvenanceCommand,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            SkillProvenanceCommand::Inspect(args) => self.cmd_provenance_inspect(args),
            SkillProvenanceCommand::Verify(args) => self.cmd_provenance_verify(args),
            SkillProvenanceCommand::Refresh(args) => self.cmd_provenance_refresh(args, request_id),
        }
    }

    fn cmd_provenance_inspect(
        &self,
        args: &SkillOnlyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let record = load_record_for_skill(&self.ctx, &args.skill)?;
        let lock = load_lock_entry_for_skill(&self.ctx, &args.skill)
            .map_err(map_io)?
            .unwrap_or(Value::Null);
        Ok((
            json!({
                "skill": args.skill,
                "provenance": record,
                "lock": lock,
            }),
            Meta::default(),
        ))
    }

    fn cmd_provenance_verify(
        &self,
        args: &SkillOnlyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let record = load_record_for_skill(&self.ctx, &args.skill)?;
        let current_digest =
            skill_tree_digest(&self.ctx.skill_path(&args.skill)).map_err(map_io)?;
        let lock = load_lock_entry_for_skill(&self.ctx, &args.skill).map_err(map_io)?;
        let lock_digest = lock
            .as_ref()
            .and_then(|entry| entry.get("digest"))
            .and_then(Value::as_str);
        let matches_record = current_digest == record.artifact.digest;
        let matches_lock = lock_digest == Some(current_digest.as_str());
        Ok((
            json!({
                "skill": args.skill,
                "matches": matches_record && matches_lock,
                "recorded_digest": record.artifact.digest,
                "current_digest": current_digest,
                "lock_digest": lock_digest,
                "lock_present": lock.is_some(),
                "source": record.source,
            }),
            Meta::default(),
        ))
    }

    fn cmd_provenance_refresh(
        &self,
        args: &SkillOnlyArgs,
        request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let _workspace = self
            .ctx
            .lock_workspace()
            .map_err(super::helpers::map_lock)?;
        self.ensure_write_repo_ready()?;
        let mut record = load_record_for_skill(&self.ctx, &args.skill)?;
        let previous_digest = record.artifact.digest.clone();
        record.artifact.digest =
            skill_tree_digest(&self.ctx.skill_path(&args.skill)).map_err(map_io)?;
        record.imported_at = Utc::now();
        record.importer_version = importer_version();
        save_record_and_lock(&self.ctx, record.clone())?;
        stage_provenance_paths(&self.ctx)?;
        let changed = previous_digest != record.artifact.digest;
        let mut meta = Meta::default();
        if gitops::has_staged_changes_for_path(&self.ctx, Path::new(SOURCES_REL))
            .map_err(map_git)?
            || gitops::has_staged_changes_for_path(&self.ctx, Path::new(LOCK_REL))
                .map_err(map_git)?
        {
            let commit = gitops::commit(
                &self.ctx,
                &format!("provenance({}): refresh lock", args.skill),
            )
            .map_err(map_git)?;
            super::projections::maybe_autosync_or_queue(
                &self.ctx,
                "provenance.refresh",
                request_id,
                json!({"skill": args.skill, "commit": commit}),
                &mut meta,
            )?;
        }
        Ok((
            json!({
                "skill": args.skill,
                "changed": changed,
                "previous_digest": previous_digest,
                "current_digest": record.artifact.digest,
                "provenance_path": sources_rel(),
                "lock_path": LOCK_REL,
            }),
            meta,
        ))
    }
}

pub(crate) fn resolve_add_source(
    ctx: &AppContext,
    args: &AddArgs,
    staging_root: &Path,
) -> std::result::Result<AddSourceResolution, CommandFailure> {
    let subdir = normalize_subdir(args.subdir.as_deref())?;
    if let Some(github) = parse_github_source(&args.source, &subdir)? {
        return clone_git_source(
            ctx,
            &github.clone_url,
            args.source_ref.as_deref(),
            github.subdir,
            staging_root,
            |commit, tree| SourceDescriptor {
                provider: "github".to_string(),
                locator: args.source.clone(),
                repository: Some(github.repository),
                path: None,
                subdir: github.lock_subdir,
                requested_ref: args.source_ref.clone(),
                resolved_commit: Some(commit),
                tree_sha: Some(tree),
            },
        );
    }

    let source_path = Path::new(&args.source);
    let looks_like_git_ref_import = args.source_ref.is_some() && source_path.exists();
    if source_path.exists() && !looks_like_git_ref_import {
        let base = fs::canonicalize(source_path).map_err(map_io)?;
        let copy_source = join_checked_subdir(&base, &subdir);
        return Ok(AddSourceResolution {
            copy_source,
            descriptor: SourceDescriptor {
                provider: "local_path".to_string(),
                locator: args.source.clone(),
                repository: None,
                path: Some(base.display().to_string()),
                subdir,
                requested_ref: None,
                resolved_commit: None,
                tree_sha: None,
            },
        });
    }

    let source = args.source.as_str();
    gitops::validate_git_url(source).map_err(|err| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("invalid git source '{}': {}", source, err),
        )
    })?;
    clone_git_source(
        ctx,
        source,
        args.source_ref.as_deref(),
        subdir.clone(),
        staging_root,
        |commit, tree| SourceDescriptor {
            provider: "git".to_string(),
            locator: args.source.clone(),
            repository: Some(args.source.clone()),
            path: None,
            subdir,
            requested_ref: args.source_ref.clone(),
            resolved_commit: Some(commit),
            tree_sha: Some(tree),
        },
    )
}

pub(crate) fn provenance_record_for_skill(
    skill: &str,
    descriptor: SourceDescriptor,
    skill_path: &Path,
) -> std::result::Result<SkillSourceRecord, CommandFailure> {
    Ok(SkillSourceRecord {
        skill_id: skill.to_string(),
        source: descriptor,
        artifact: ArtifactDescriptor {
            digest: skill_tree_digest(skill_path).map_err(map_io)?,
        },
        imported_at: Utc::now(),
        importer_version: importer_version(),
    })
}

pub(crate) fn save_record_and_lock(
    ctx: &AppContext,
    record: SkillSourceRecord,
) -> std::result::Result<(), CommandFailure> {
    let mut sources = load_sources(ctx).map_err(map_io)?;
    sources
        .sources
        .retain(|item| item.skill_id != record.skill_id);
    sources.sources.push(record);
    sources.sources.sort_by(|a, b| a.skill_id.cmp(&b.skill_id));
    write_sources(ctx, &sources).map_err(map_io)?;
    write_lock(ctx, &sources).map_err(map_io)?;
    Ok(())
}

pub(crate) fn stage_provenance_paths(ctx: &AppContext) -> std::result::Result<(), CommandFailure> {
    gitops::stage_path(ctx, Path::new(SOURCES_REL)).map_err(map_git)?;
    gitops::stage_path(ctx, Path::new(LOCK_REL)).map_err(map_git)?;
    Ok(())
}

pub(crate) fn provenance_digest_status(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<Option<ProvenanceDigestStatus>, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let sources = load_sources(ctx).map_err(map_io)?;
    let Some(record) = sources
        .sources
        .into_iter()
        .find(|record| record.skill_id == skill)
    else {
        return Ok(None);
    };
    let current_digest = skill_tree_digest(&ctx.skill_path(skill)).map_err(map_io)?;
    let lock = load_lock_entry_for_skill(ctx, skill).map_err(map_io)?;
    let lock_digest = lock
        .as_ref()
        .and_then(|entry| entry.get("digest"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let matches_record = current_digest == record.artifact.digest;
    let matches_lock = lock_digest.as_deref() == Some(current_digest.as_str());
    Ok(Some(ProvenanceDigestStatus {
        recorded_digest: record.artifact.digest,
        current_digest,
        lock_digest,
        lock_present: lock.is_some(),
        matches: matches_record && matches_lock,
    }))
}

fn load_record_for_skill(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<SkillSourceRecord, CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    if !ctx.skill_path(skill).exists() {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ));
    }
    let sources = load_sources(ctx).map_err(map_io)?;
    sources
        .sources
        .into_iter()
        .find(|record| record.skill_id == skill)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateNotInitialized,
                format!("provenance for skill '{}' not found", skill),
            )
        })
}

fn load_sources(ctx: &AppContext) -> Result<SkillSourcesFile> {
    let path = sources_file(ctx);
    if !path.exists() {
        return Ok(SkillSourcesFile {
            schema_version: 1,
            sources: Vec::new(),
        });
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

fn load_lock_entry_for_skill(ctx: &AppContext, skill: &str) -> Result<Option<Value>> {
    let path = ctx.root.join(LOCK_REL);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let lock: LoomLockFile =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    lock.skills
        .get(skill)
        .map(serde_json::to_value)
        .transpose()
        .map_err(Into::into)
}

fn write_sources(ctx: &AppContext, sources: &SkillSourcesFile) -> Result<()> {
    let path = sources_file(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(sources)? + "\n";
    fs::write(&path, raw).with_context(|| format!("write {}", path.display()))
}

fn write_lock(ctx: &AppContext, sources: &SkillSourcesFile) -> Result<()> {
    let mut skills = BTreeMap::new();
    for record in &sources.sources {
        skills.insert(record.skill_id.clone(), lock_skill_for_record(ctx, record)?);
    }
    let lock = LoomLockFile { version: 1, skills };
    let raw = serde_json::to_string_pretty(&lock)? + "\n";
    fs::write(ctx.root.join(LOCK_REL), raw).with_context(|| format!("write {}", LOCK_REL))
}

fn lock_skill_for_record(ctx: &AppContext, record: &SkillSourceRecord) -> Result<LoomLockSkill> {
    Ok(LoomLockSkill {
        source: lock_source_locator(&record.source),
        provider: record.source.provider.clone(),
        requested_ref: record.source.requested_ref.clone(),
        commit: record.source.resolved_commit.clone(),
        tree_sha: record.source.tree_sha.clone(),
        digest: record.artifact.digest.clone(),
        agents: projected_agents(ctx, &record.skill_id)?,
        scope: "project".to_string(),
    })
}

fn projected_agents(ctx: &AppContext, skill: &str) -> Result<Vec<String>> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let Some(snapshot) = paths.maybe_load_snapshot()? else {
        return Ok(Vec::new());
    };
    let mut target_ids = BTreeSet::new();
    for projection in snapshot.projections.projections {
        if projection.skill_id == skill {
            target_ids.insert(projection.target_id);
        }
    }
    for rule in snapshot.rules.rules {
        if rule.skill_id == skill {
            target_ids.insert(rule.target_id);
        }
    }
    let mut agents = BTreeSet::new();
    for target in snapshot.targets.targets {
        if target_ids.contains(&target.target_id) {
            agents.insert(target.agent);
        }
    }
    Ok(agents.into_iter().collect())
}

fn lock_source_locator(source: &SourceDescriptor) -> String {
    match source.provider.as_str() {
        "github" => format!(
            "github:{}//{}",
            source.repository.as_deref().unwrap_or_default(),
            source.subdir
        ),
        "git" => format!(
            "{}//{}",
            source.repository.as_deref().unwrap_or(&source.locator),
            source.subdir
        ),
        "local_path" => format!(
            "{}//{}",
            source.path.as_deref().unwrap_or(&source.locator),
            source.subdir
        ),
        _ => source.locator.clone(),
    }
}

fn sources_file(ctx: &AppContext) -> PathBuf {
    ctx.root.join(SOURCES_REL)
}

fn sources_rel() -> &'static str {
    SOURCES_REL
}

fn importer_version() -> String {
    format!("loom/{}", env!("CARGO_PKG_VERSION"))
}

pub(crate) fn skill_tree_digest(path: &Path) -> Result<String> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(path).follow_links(false).sort_by_file_name() {
        let entry = entry.with_context(|| format!("walk {}", path.display()))?;
        if entry.file_type().is_dir() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(path)
            .with_context(|| format!("strip {}", path.display()))?;
        entries.push((rel.to_path_buf(), entry.path().to_path_buf()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel, full) in entries {
        let rel = rel.to_string_lossy();
        hasher.update(b"path\0");
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        let metadata =
            fs::symlink_metadata(&full).with_context(|| format!("stat {}", full.display()))?;
        if metadata.file_type().is_symlink() {
            hasher.update(b"symlink\0");
            hasher.update(fs::read_link(&full)?.to_string_lossy().as_bytes());
        } else {
            hasher.update(b"file\0");
            let mut file =
                fs::File::open(&full).with_context(|| format!("open {}", full.display()))?;
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .with_context(|| format!("read {}", full.display()))?;
            hasher.update(&(buf.len() as u64).to_be_bytes());
            hasher.update(&buf);
        }
        hasher.update(b"\0");
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

struct GithubSource {
    repository: String,
    clone_url: String,
    subdir: String,
    lock_subdir: String,
}

fn parse_github_source(
    raw: &str,
    arg_subdir: &str,
) -> std::result::Result<Option<GithubSource>, CommandFailure> {
    let Some(rest) = raw.strip_prefix("github:") else {
        return Ok(None);
    };
    let (repo, source_subdir) = rest.split_once("//").unwrap_or((rest, ""));
    if repo.is_empty() || !repo.contains('/') {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "github source must look like github:owner/repo//optional/subdir",
        ));
    }
    if !source_subdir.is_empty() && !arg_subdir.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "pass subdir either in github:owner/repo//subdir or --subdir, not both",
        ));
    }
    let source_subdir = if source_subdir.is_empty() {
        arg_subdir.to_string()
    } else {
        normalize_subdir(Some(Path::new(source_subdir)))?
    };
    Ok(Some(GithubSource {
        repository: repo.to_string(),
        clone_url: format!("https://github.com/{}.git", repo),
        subdir: source_subdir.clone(),
        lock_subdir: source_subdir,
    }))
}

fn clone_git_source<F>(
    _ctx: &AppContext,
    source: &str,
    requested_ref: Option<&str>,
    subdir: String,
    staging_root: &Path,
    descriptor: F,
) -> std::result::Result<AddSourceResolution, CommandFailure>
where
    F: FnOnce(String, String) -> SourceDescriptor,
{
    let clone_tmp = staging_root.join("clone");
    remove_path_if_exists(&clone_tmp).map_err(map_io)?;
    let clone = run_git_allow_failure_in(
        staging_root,
        &["clone", source, clone_tmp.to_string_lossy().as_ref()],
    )
    .map_err(map_git)?;
    if !clone.status.success() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "failed to clone source: {}",
                String::from_utf8_lossy(&clone.stderr).trim()
            ),
        ));
    }
    if let Some(reference) = requested_ref {
        let checkout = run_git_allow_failure_in(&clone_tmp, &["checkout", "--detach", reference])
            .map_err(map_git)?;
        if !checkout.status.success() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "failed to checkout ref '{}': {}",
                    reference,
                    String::from_utf8_lossy(&checkout.stderr).trim()
                ),
            ));
        }
    }
    let commit = run_git_in(&clone_tmp, &["rev-parse", "HEAD"]).map_err(map_git)?;
    let tree_ref = if subdir.is_empty() {
        "HEAD^{tree}".to_string()
    } else {
        format!("HEAD:{}", subdir)
    };
    let tree = run_git_in(&clone_tmp, &["rev-parse", &tree_ref]).map_err(map_git)?;
    let copy_source = join_checked_subdir(&clone_tmp, &subdir);
    Ok(AddSourceResolution {
        copy_source,
        descriptor: descriptor(commit, tree),
    })
}

fn normalize_subdir(value: Option<&Path>) -> std::result::Result<String, CommandFailure> {
    let Some(path) = value else {
        return Ok(String::new());
    };
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "--subdir must be a relative path without '..'",
                ));
            }
        }
    }
    Ok(parts.join("/"))
}

fn join_checked_subdir(base: &Path, subdir: &str) -> PathBuf {
    if subdir.is_empty() {
        base.to_path_buf()
    } else {
        base.join(subdir)
    }
}

fn run_git_in(repo_dir: &Path, args: &[&str]) -> Result<String> {
    let output = run_git_allow_failure_in(repo_dir, args)?;
    if !output.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git_allow_failure_in(repo_dir: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(repo_dir)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("protocol.file.allow=always")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {:?}", args))
}
