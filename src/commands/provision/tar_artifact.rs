use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tar::{Archive, Builder, EntryType, Header};
use uuid::Uuid;

use crate::commands::CommandFailure;
use crate::commands::helpers::map_io;
use crate::fs_util::rename_atomic;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::artifact::ProvisionArtifactInspection;
use super::model::ProvisionPlan;
use super::utils::shell_safe_segment;

const TAR_ARTIFACT_SCHEMA: &str = "provision-tar-artifact-v1";

pub(super) struct TarExportArtifact {
    pub schema_version: &'static str,
    pub entry_count: usize,
    pub generated_file_count: usize,
    pub registry_file_count: usize,
    pub active_view_file_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct TarManifest {
    schema_version: String,
    plan_schema_version: String,
    plan_id: String,
    target_kind: String,
    created_at: String,
    generated_files: Vec<TarGeneratedFile>,
    registry_files: Vec<TarRegistryFile>,
    active_view_files: Vec<TarActiveViewFile>,
    active_views: Value,
    target_writes_performed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct TarGeneratedFile {
    target_path: String,
    archive_path: String,
    kind: String,
    content_digest: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TarRegistryFile {
    skill: String,
    source_path: String,
    archive_path: String,
    content_digest: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TarActiveViewFile {
    agent: String,
    skill: String,
    source_path: String,
    archive_path: String,
    reviewed_target_path: String,
    content_digest: String,
}

pub(super) fn build_tar_export_artifact(
    ctx: &AppContext,
    plan: &ProvisionPlan,
    output: &Path,
) -> Result<TarExportArtifact, CommandFailure> {
    let root = format!("loom-provision-{}", plan.plan_id);
    let mtime = plan.created_at.timestamp().max(0) as u64;
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    let mut generated_files = Vec::new();
    for file in &plan.files_to_write {
        let archive_path = format!("files/{}", file.path);
        insert_entry(
            &mut entries,
            &archive_path,
            file.preview.as_bytes().to_vec(),
        )?;
        generated_files.push(TarGeneratedFile {
            target_path: file.path.clone(),
            archive_path,
            kind: file.kind.clone(),
            content_digest: file.content_digest.clone(),
        });
    }

    let registry_files = collect_registry_files(ctx, plan)?;
    for file in &registry_files {
        insert_entry(
            &mut entries,
            &file.archive_path,
            fs::read(
                ctx.root
                    .join("skills")
                    .join(&file.skill)
                    .join(&file.source_path),
            )
            .map_err(map_io)?,
        )?;
    }

    let active_view_files = collect_active_view_files(plan, &registry_files)?;
    for file in &active_view_files {
        let bytes = entries
            .get(&format!(
                "registry/skills/{}/{}",
                safe_archive_segment(&file.skill)?,
                file.source_path
            ))
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    "active-view file source is missing from registry artifact entries",
                )
            })?
            .clone();
        insert_entry(&mut entries, &file.archive_path, bytes)?;
    }

    let active_views_bytes = serde_json::to_vec_pretty(&plan.active_views).map_err(map_io)?;
    insert_entry(
        &mut entries,
        "active_views.json",
        newline(active_views_bytes),
    )?;
    let plan_bytes = serde_json::to_vec_pretty(plan).map_err(map_io)?;
    insert_entry(&mut entries, "plan.json", newline(plan_bytes))?;
    let manifest = TarManifest {
        schema_version: TAR_ARTIFACT_SCHEMA.to_string(),
        plan_schema_version: plan.schema_version.clone(),
        plan_id: plan.plan_id.clone(),
        target_kind: plan.target_kind.clone(),
        created_at: plan.created_at.to_rfc3339(),
        generated_files,
        registry_files,
        active_view_files,
        active_views: serde_json::to_value(&plan.active_views).map_err(map_io)?,
        target_writes_performed: false,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(map_io)?;
    insert_entry(&mut entries, "manifest.json", newline(manifest_bytes))?;
    let checksums = checksums_bytes(&entries);
    insert_entry(&mut entries, "checksums.txt", checksums)?;
    write_tar(output, &root, &entries, mtime)?;

    Ok(TarExportArtifact {
        schema_version: TAR_ARTIFACT_SCHEMA,
        entry_count: entries.len(),
        generated_file_count: manifest.generated_files.len(),
        registry_file_count: manifest.registry_files.len(),
        active_view_file_count: manifest.active_view_files.len(),
    })
}

pub(super) fn inspect_tar_export_artifact(
    path: &Path,
) -> Result<ProvisionArtifactInspection, CommandFailure> {
    let file = File::open(path).map_err(map_io)?;
    let mut archive = Archive::new(file);
    let mut root_name: Option<String> = None;
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    for entry in archive.entries().map_err(map_io)? {
        let mut entry = entry.map_err(map_io)?;
        let path = entry.path().map_err(map_io)?.into_owned();
        validate_tar_entry(entry.header().entry_type(), &path)?;
        let normalized = normalize_archive_path(&path)?;
        let mut parts = normalized.splitn(2, '/');
        let root = parts.next().unwrap_or_default();
        let rel = parts.next().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                "provision tar artifact entries must live under one root directory",
            )
        })?;
        if let Some(existing) = &root_name {
            if existing != root {
                return Err(invalid_artifact(
                    "provision tar artifact contains multiple root directories",
                ));
            }
        } else {
            root_name = Some(root.to_string());
        }
        if entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(map_io)?;
            insert_entry(&mut entries, rel, bytes)?;
        }
    }
    verify_checksums(&entries)?;
    let manifest_bytes = entries
        .get("manifest.json")
        .ok_or_else(|| invalid_artifact("provision tar artifact missing manifest.json"))?;
    let manifest: TarManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))?;
    if manifest.schema_version != TAR_ARTIFACT_SCHEMA {
        return Err(invalid_artifact(
            "unsupported provision tar artifact schema",
        ));
    }
    verify_manifest_entries(&entries, &manifest)?;

    Ok(ProvisionArtifactInspection {
        kind: "tar",
        schema_version: manifest.schema_version,
        plan_id: manifest.plan_id,
        target_kind: manifest.target_kind,
        source_path: None,
        content_digest: None,
        script_bytes: None,
        checksums_verified: true,
        entry_count: entries.len(),
        generated_file_count: manifest.generated_files.len(),
        registry_file_count: manifest.registry_files.len(),
        active_view_file_count: manifest.active_view_files.len(),
        planned_files: json!(
            manifest
                .generated_files
                .iter()
                .map(|file| json!({
                    "path": file.target_path,
                    "kind": file.kind,
                    "content_digest": file.content_digest,
                    "action": "review_only",
                }))
                .collect::<Vec<_>>()
        ),
    })
}

fn collect_registry_files(
    ctx: &AppContext,
    plan: &ProvisionPlan,
) -> Result<Vec<TarRegistryFile>, CommandFailure> {
    let mut files = Vec::new();
    for skill in active_skill_ids(plan) {
        let skill_root = ctx.root.join("skills").join(&skill);
        if !skill_root.is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("plan references skill '{skill}' but registry source is missing"),
            ));
        }
        let mut relative_files = Vec::new();
        collect_regular_files(&skill_root, &skill_root, &mut relative_files)?;
        let skill_segment = safe_archive_segment(&skill)?;
        for relative in relative_files {
            let source_path = archive_relative_path(&relative)?;
            let bytes = fs::read(skill_root.join(&relative)).map_err(map_io)?;
            files.push(TarRegistryFile {
                skill: skill.clone(),
                source_path: source_path.clone(),
                archive_path: format!("registry/skills/{skill_segment}/{source_path}"),
                content_digest: digest_bytes(&bytes),
            });
        }
    }
    Ok(files)
}

fn collect_active_view_files(
    plan: &ProvisionPlan,
    registry_files: &[TarRegistryFile],
) -> Result<Vec<TarActiveViewFile>, CommandFailure> {
    let mut files = Vec::new();
    for (view_index, view) in plan.active_views.iter().enumerate() {
        let agent_segment = safe_archive_segment(&view.agent)?;
        let view_segment = format!("{agent_segment}-{view_index}");
        for skill in &view.skills {
            let skill_segment = safe_archive_segment(skill)?;
            for source in registry_files.iter().filter(|file| file.skill == *skill) {
                let archive_path = format!(
                    "active-views/{view_segment}/{skill_segment}/{}",
                    source.source_path
                );
                files.push(TarActiveViewFile {
                    agent: view.agent.clone(),
                    skill: skill.clone(),
                    source_path: source.source_path.clone(),
                    archive_path,
                    reviewed_target_path: format!(
                        "{}/{}/{}",
                        view.path.trim_end_matches('/'),
                        skill_segment,
                        source.source_path
                    ),
                    content_digest: source.content_digest.clone(),
                });
            }
        }
    }
    Ok(files)
}

fn active_skill_ids(plan: &ProvisionPlan) -> BTreeSet<String> {
    plan.active_views
        .iter()
        .flat_map(|view| view.skills.iter().cloned())
        .collect()
}

fn collect_regular_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), CommandFailure> {
    let mut entries = fs::read_dir(current)
        .map_err(map_io)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_io)?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.components().any(|component| {
            matches!(component, Component::Normal(name) if name.to_string_lossy() == ".git")
        }) {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "provision tar export refuses to include .git metadata",
            ));
        }
        let file_type = entry.file_type().map_err(map_io)?;
        if file_type.is_symlink() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "provision tar export refuses symlinked skill source path {}",
                    path.display()
                ),
            ));
        }
        if file_type.is_dir() {
            collect_regular_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(map_io)?.to_path_buf();
            files.push(relative);
        }
    }
    Ok(())
}

fn write_tar(
    output: &Path,
    root: &str,
    entries: &BTreeMap<String, Vec<u8>>,
    mtime: u64,
) -> Result<(), CommandFailure> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(map_io)?;
    let temp_output = parent.join(format!(
        ".{}.tmp-{}",
        output.file_name().unwrap_or_default().to_string_lossy(),
        Uuid::new_v4()
    ));
    let result = (|| {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_output)
            .map_err(map_io)?;
        let mut builder = Builder::new(file);
        for (relative, bytes) in entries {
            append_tar_bytes(&mut builder, &Path::new(root).join(relative), bytes, mtime)
                .map_err(map_io)?;
        }
        builder.finish().map_err(map_io)?;
        rename_atomic(&temp_output, output).map_err(map_io)
    })();
    if result.is_err() {
        match fs::remove_file(&temp_output) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(map_io(err)),
        }
    }
    result
}

fn append_tar_bytes(
    builder: &mut Builder<File>,
    archive_path: &Path,
    bytes: &[u8],
    mtime: u64,
) -> std::io::Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(mtime);
    header.set_cksum();
    builder.append_data(&mut header, archive_path, Cursor::new(bytes))
}

fn insert_entry(
    entries: &mut BTreeMap<String, Vec<u8>>,
    path: &str,
    bytes: Vec<u8>,
) -> Result<(), CommandFailure> {
    validate_archive_relative(path)?;
    if entries.insert(path.to_string(), bytes).is_some() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("provision artifact contains duplicate entry {path}"),
        ));
    }
    Ok(())
}

fn verify_manifest_entries(
    entries: &BTreeMap<String, Vec<u8>>,
    manifest: &TarManifest,
) -> Result<(), CommandFailure> {
    let mut expected = BTreeSet::from([
        "active_views.json".to_string(),
        "checksums.txt".to_string(),
        "manifest.json".to_string(),
        "plan.json".to_string(),
    ]);
    for file in &manifest.generated_files {
        expected.insert(file.archive_path.clone());
        verify_entry_digest(entries, &file.archive_path, &file.content_digest)?;
    }
    for file in &manifest.registry_files {
        expected.insert(file.archive_path.clone());
        verify_entry_digest(entries, &file.archive_path, &file.content_digest)?;
    }
    for file in &manifest.active_view_files {
        expected.insert(file.archive_path.clone());
        verify_entry_digest(entries, &file.archive_path, &file.content_digest)?;
    }
    let actual = entries.keys().cloned().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid_artifact(
            "provision tar artifact entries do not match manifest",
        ));
    }
    Ok(())
}

fn verify_entry_digest(
    entries: &BTreeMap<String, Vec<u8>>,
    path: &str,
    expected: &str,
) -> Result<(), CommandFailure> {
    let bytes = entries
        .get(path)
        .ok_or_else(|| invalid_artifact(format!("provision tar artifact missing {path}")))?;
    let actual = digest_bytes(bytes);
    if actual != expected {
        return Err(invalid_artifact(format!(
            "provision tar artifact digest mismatch for {path}"
        )));
    }
    Ok(())
}

fn checksums_bytes(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut raw = String::new();
    for (path, bytes) in entries {
        raw.push_str(&format!("{}  {}\n", digest_bytes(bytes), path));
    }
    raw.into_bytes()
}

fn verify_checksums(entries: &BTreeMap<String, Vec<u8>>) -> Result<(), CommandFailure> {
    let raw = entries
        .get("checksums.txt")
        .ok_or_else(|| invalid_artifact("provision tar artifact missing checksums.txt"))?;
    let text = std::str::from_utf8(raw)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))?;
    let mut expected = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some((hash, path)) = line.split_once("  ") else {
            return Err(invalid_artifact(
                "provision tar artifact has malformed checksums.txt",
            ));
        };
        expected.insert(path.to_string(), hash.to_string());
    }
    for (path, bytes) in entries {
        if path == "checksums.txt" {
            continue;
        }
        let Some(expected_hash) = expected.get(path) else {
            return Err(invalid_artifact(format!(
                "provision tar artifact checksum missing for {path}"
            )));
        };
        if digest_bytes(bytes) != *expected_hash {
            return Err(invalid_artifact(format!(
                "provision tar artifact checksum mismatch for {path}"
            )));
        }
    }
    Ok(())
}

fn validate_tar_entry(entry_type: EntryType, path: &Path) -> Result<(), CommandFailure> {
    if entry_type.is_file() || entry_type == EntryType::Directory {
        normalize_archive_path(path)?;
        return Ok(());
    }
    Err(invalid_artifact(
        "provision tar artifact may contain only regular files and directories",
    ))
}

fn normalize_archive_path(path: &Path) -> Result<String, CommandFailure> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.is_empty() {
                    return Err(invalid_artifact(
                        "provision tar artifact has empty path part",
                    ));
                }
                parts.push(part.to_string());
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(invalid_artifact(
                    "provision tar artifact contains an unsafe path",
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(invalid_artifact("provision tar artifact has an empty path"));
    }
    Ok(parts.join("/"))
}

fn archive_relative_path(path: &Path) -> Result<String, CommandFailure> {
    let raw = path
        .components()
        .map(|component| match component {
            Component::Normal(part) => Ok(part.to_string_lossy().to_string()),
            Component::CurDir => Ok(String::new()),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => Err(
                CommandFailure::new(ErrorCode::ArgInvalid, "unsafe provision artifact path"),
            ),
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    validate_archive_relative(&raw)?;
    Ok(raw)
}

fn validate_archive_relative(path: &str) -> Result<(), CommandFailure> {
    if path.is_empty() || path.starts_with('/') || path.split('/').any(|part| part == "..") {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsafe provision artifact path {path}"),
        ));
    }
    Ok(())
}

fn safe_archive_segment(value: &str) -> Result<String, CommandFailure> {
    let segment = shell_safe_segment(value);
    if segment.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provision artifact path segment is empty after sanitization",
        ));
    }
    Ok(segment)
}

fn newline(mut bytes: Vec<u8>) -> Vec<u8> {
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    format!("sha256:{}", to_hex(&hash.finalize()))
}

fn invalid_artifact(message: impl Into<String>) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, message.into())
}
