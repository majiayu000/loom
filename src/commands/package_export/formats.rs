use std::collections::BTreeMap;

use serde_json::json;

use super::model::{PackageFilePlan, PackageSource, digest_bytes};
use super::{CommandFailure, ErrorCode, map_io};

pub(super) const FORMATS: &[&str] = &[
    "agent-skills-archive",
    "codex-plugin",
    "claude-plugin",
    "npm",
    "github-release",
];

pub(super) fn generated_files(
    format: &str,
    source: &PackageSource,
    digest: &str,
) -> Result<BTreeMap<String, Vec<u8>>, CommandFailure> {
    let mut files = BTreeMap::new();
    if format == "agent-skills-archive" {
        return Ok(files);
    }
    if !FORMATS.contains(&format) {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsupported package format '{format}'"),
        ));
    }
    if source.id.is_empty()
        || source.id.len() > 64
        || source.id.starts_with('-')
        || source.id.ends_with('-')
        || !source
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "native package names must be lowercase kebab-case and at most 64 bytes",
        ));
    }
    let hash = digest
        .strip_prefix("sha256:")
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            CommandFailure::new(ErrorCode::StateCorrupt, "invalid package source digest")
        })?;
    let version = format!("0.0.0-source-{}", &hash[..12]);
    let description = source
        .description
        .clone()
        .unwrap_or_else(|| format!("Skills from {}", source.id));
    let (path, metadata) = match format {
        "codex-plugin" => (
            ".codex-plugin/plugin.json",
            json!({"name": source.id, "version": version, "description": description, "skills": "./skills/"}),
        ),
        "claude-plugin" => (
            ".claude-plugin/plugin.json",
            json!({"name": source.id, "version": version, "description": description}),
        ),
        "npm" => (
            "package.json",
            json!({"name": source.id, "version": version, "description": description, "files": ["skills/", "manifest.json", "provenance.json", "checksums.txt"]}),
        ),
        "github-release" => (
            "release.json",
            json!({"name": source.id, "source_digest": digest, "checksums": "checksums.txt", "provenance": "provenance.json", "publish_performed": false}),
        ),
        _ => unreachable!("checked supported format"),
    };
    let mut bytes = serde_json::to_vec_pretty(&metadata).map_err(map_io)?;
    bytes.push(b'\n');
    files.insert(path.to_string(), bytes);
    Ok(files)
}

pub(super) fn extend_plan(files: &mut Vec<PackageFilePlan>, generated: &BTreeMap<String, Vec<u8>>) {
    files.extend(generated.iter().map(|(path, bytes)| PackageFilePlan {
        path: path.clone(),
        kind: "generated".to_string(),
        size: bytes.len() as u64,
        sha256: digest_bytes(bytes),
    }));
}
