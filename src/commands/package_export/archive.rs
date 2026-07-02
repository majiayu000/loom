use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tar::{Archive, Builder, EntryType, Header};

use crate::cli::{PackageFormatArg, PackageVerifyArgs};
use crate::fs_util::rename_atomic;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::model::{
    CopyFile, PackageFilePlan, PackageManifest, PackageTempDir, SUPPORTED_FORMAT, digest_bytes,
};
use super::source::{
    collect_source_files, package_policy_blocked, reject_forbidden_content, source_digest,
    validate_package_relative_path,
};
use super::{
    CommandFailure, PACKAGE_SCHEMA_VERSION, SkillLintMode, ensure_supported_format, map_io,
    package_format_as_str,
};
use crate::commands::lint_skill_source;

pub(super) fn build_archive(
    output: &Path,
    artifact_root: &str,
    manifest: &PackageManifest,
    copy_files: &[CopyFile],
) -> std::result::Result<(), CommandFailure> {
    let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
    if output.exists() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("package artifact already exists: {}", output.display()),
        ));
    }
    let staging = PackageTempDir::new_in(output_parent, ".loom-package-output").map_err(map_io)?;
    let temp_output = staging.path.join("artifact.tar");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_output)
        .map_err(map_io)?;

    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    let mut manifest_bytes = serde_json::to_vec_pretty(manifest).map_err(map_io)?;
    manifest_bytes.push(b'\n');
    entries.insert("manifest.json".to_string(), manifest_bytes);
    entries.insert(
        "provenance.json".to_string(),
        provenance_bytes(manifest).map_err(map_io)?,
    );
    for file in copy_files {
        entries.insert(file.archive_rel.clone(), file.bytes.clone());
    }
    let checksums = checksums_bytes(&entries);
    entries.insert("checksums.txt".to_string(), checksums);

    let mut builder = Builder::new(file);
    let mtime = manifest.created_at.timestamp().max(0) as u64;
    for (rel, bytes) in entries {
        append_bytes(
            &mut builder,
            &Path::new(artifact_root).join(rel),
            &bytes,
            mtime,
        )
        .map_err(map_io)?;
    }
    builder.finish().map_err(map_io)?;
    if output.exists() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("package artifact already exists: {}", output.display()),
        ));
    }
    rename_atomic(&temp_output, output).map_err(map_io)
}

pub(super) fn verify_archive(
    ctx: &AppContext,
    args: &PackageVerifyArgs,
) -> std::result::Result<Value, CommandFailure> {
    if !args.artifact.is_file() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "package artifact does not exist: {}",
                args.artifact.display()
            ),
        ));
    }
    let temp = PackageTempDir::new("loom-package-verify").map_err(map_io)?;
    let file = File::open(&args.artifact).map_err(map_io)?;
    let mut archive = Archive::new(file);
    let mut entries = BTreeMap::<String, Vec<u8>>::new();
    let mut root_name: Option<String> = None;
    for entry in archive.entries().map_err(map_io)? {
        let mut entry = entry.map_err(map_io)?;
        let path = entry.path().map_err(map_io)?.into_owned();
        package_validate_archive_entry(entry.header().entry_type(), &path)?;
        let normalized = normalized_archive_path(&path)?;
        let mut components = normalized.splitn(2, '/');
        let root = components.next().unwrap_or_default();
        let rel = components.next().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                "package artifact entries must live under one root directory",
            )
        })?;
        if let Some(existing) = &root_name {
            if existing != root {
                return Err(CommandFailure::new(
                    ErrorCode::StateCorrupt,
                    "package artifact contains multiple top-level directories",
                ));
            }
        } else {
            root_name = Some(root.to_string());
        }
        if entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(map_io)?;
            reject_forbidden_content(ctx, rel, &bytes)?;
            let extracted = temp.path.join(&normalized);
            if let Some(parent) = extracted.parent() {
                std::fs::create_dir_all(parent).map_err(map_io)?;
            }
            std::fs::write(&extracted, &bytes).map_err(map_io)?;
            entries.insert(rel.to_string(), bytes);
        }
    }
    let manifest_bytes = entries.get("manifest.json").ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "package artifact missing manifest.json",
        )
    })?;
    let manifest: PackageManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|err| CommandFailure::new(ErrorCode::StateCorrupt, err.to_string()))?;
    verify_checksums(&entries)?;
    validate_manifest(ctx, args.format, &manifest)?;
    verify_manifest_file_list(&entries, &manifest)?;
    let source_fresh = verify_source_freshness(ctx, &manifest)?;
    verify_lint(temp.path.join(root_name.unwrap_or_default()), &manifest)?;
    Ok(json!({
        "artifact": args.artifact.display().to_string(),
        "valid": true,
        "format": manifest.format,
        "manifest": manifest,
        "checksums_verified": true,
        "source_fresh": source_fresh,
    }))
}

fn validate_manifest(
    ctx: &AppContext,
    expected: Option<PackageFormatArg>,
    manifest: &PackageManifest,
) -> std::result::Result<(), CommandFailure> {
    if manifest.schema_version != PACKAGE_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported package manifest schema_version {}",
                manifest.schema_version
            ),
        ));
    }
    if manifest.format != SUPPORTED_FORMAT {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsupported package artifact format '{}'", manifest.format),
        ));
    }
    if let Some(expected) = expected {
        ensure_supported_format(expected)?;
        if manifest.format != package_format_as_str(expected) {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "package artifact format does not match --format",
            ));
        }
    }
    for file in &manifest.files {
        reject_forbidden_content(ctx, &file.path, file.path.as_bytes())?;
    }
    Ok(())
}

fn verify_checksums(
    entries: &BTreeMap<String, Vec<u8>>,
) -> std::result::Result<(), CommandFailure> {
    let raw = entries.get("checksums.txt").ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            "package artifact missing checksums.txt",
        )
    })?;
    let text = std::str::from_utf8(raw)
        .map_err(|err| CommandFailure::new(ErrorCode::StateCorrupt, err.to_string()))?;
    let mut expected = BTreeMap::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some((hash, path)) = line.split_once("  ") else {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                "malformed checksums.txt line",
            ));
        };
        expected.insert(path.to_string(), hash.to_string());
    }
    for (path, bytes) in entries {
        if path == "checksums.txt" {
            continue;
        }
        let Some(expected_hash) = expected.get(path) else {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("checksums.txt missing entry for {path}"),
            ));
        };
        let actual = digest_bytes(bytes);
        if actual != *expected_hash {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("checksum mismatch for {path}"),
            ));
        }
    }
    Ok(())
}

fn verify_manifest_file_list(
    entries: &BTreeMap<String, Vec<u8>>,
    manifest: &PackageManifest,
) -> std::result::Result<(), CommandFailure> {
    let mut expected = BTreeMap::<String, &PackageFilePlan>::new();
    for file in &manifest.files {
        if expected.insert(file.path.clone(), file).is_some() {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("manifest contains duplicate package path {}", file.path),
            ));
        }
    }
    for path in entries.keys() {
        if !expected.contains_key(path) {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("package artifact contains extra file {path}"),
            ));
        }
    }
    for (path, file) in expected {
        let Some(bytes) = entries.get(&path) else {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("package artifact missing manifest file {path}"),
            ));
        };
        if bytes.len() as u64 != file.size && file.kind == "copied" {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("manifest size mismatch for {path}"),
            ));
        }
        if file.kind == "copied" && digest_bytes(bytes) != file.sha256 {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("manifest checksum mismatch for {path}"),
            ));
        }
        if file.kind == "generated"
            && !matches!(
                path.as_str(),
                "manifest.json" | "provenance.json" | "checksums.txt"
            )
        {
            return Err(CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("manifest declares unsupported generated file {path}"),
            ));
        }
    }
    Ok(())
}

fn verify_source_freshness(
    ctx: &AppContext,
    manifest: &PackageManifest,
) -> std::result::Result<&'static str, CommandFailure> {
    let files = match collect_source_files(ctx, &manifest.source) {
        Ok(files) => files,
        Err(err) if matches!(err.code, ErrorCode::SkillNotFound) => return Ok("unknown"),
        Err(err) => return Err(err),
    };
    let digest = source_digest(&manifest.source, &files);
    if digest != manifest.source_digest {
        return Err(package_policy_blocked(
            "package source digest is stale",
            json!({"expected": manifest.source_digest, "actual": digest}),
        ));
    }
    Ok("pass")
}

fn verify_lint(
    root: PathBuf,
    manifest: &PackageManifest,
) -> std::result::Result<(), CommandFailure> {
    for member in &manifest.source.members {
        let skill_path = root.join("skills").join(&member.skill_id);
        let lint = lint_skill_source(&skill_path, &member.skill_id, SkillLintMode::Strict);
        if !lint.valid {
            return Err(package_policy_blocked(
                "packaged skill failed portable lint",
                json!({"skill": member.skill_id, "lint": lint}),
            ));
        }
    }
    Ok(())
}

fn append_bytes(
    builder: &mut Builder<File>,
    archive_path: &Path,
    bytes: &[u8],
    mtime: u64,
) -> io::Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(0o644);
    header.set_mtime(mtime);
    header.set_size(bytes.len() as u64);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    builder.append_data(&mut header, archive_path, Cursor::new(bytes))
}

fn provenance_bytes(manifest: &PackageManifest) -> serde_json::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(&json!({
        "schema_version": PACKAGE_SCHEMA_VERSION,
        "plan_id": manifest.plan_id,
        "source": manifest.source,
        "format": manifest.format,
        "source_ref": manifest.source_ref,
        "source_digest": manifest.source_digest,
        "loom_version": manifest.loom_version,
        "local_paths_redacted": true,
    }))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn checksums_bytes(entries: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut out = String::new();
    for (path, bytes) in entries {
        out.push_str(&format!("{}  {}\n", digest_bytes(bytes), path));
    }
    out.into_bytes()
}

fn package_validate_archive_entry(
    entry_type: EntryType,
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    validate_package_relative_path(path)?;
    if !entry_type.is_file() {
        return Err(package_policy_blocked(
            "package artifact contains an unsupported entry type",
            json!({"path": path.display().to_string(), "entry_type": format!("{entry_type:?}")}),
        ));
    }
    Ok(())
}

fn normalized_archive_path(path: &Path) -> std::result::Result<String, CommandFailure> {
    validate_package_relative_path(path)?;
    Ok(path.to_string_lossy().replace('\\', "/"))
}
