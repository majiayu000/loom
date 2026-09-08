//! Authenticated download is owned by the desktop. No credentials enter this boundary.
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use super::provenance::SourceDescriptor;
use crate::sha256::{Sha256, to_hex};

const COMPRESSED_LIMIT: u64 = 10 * 1024 * 1024;
const EXPANDED_LIMIT: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeamArtifactManifest {
    pub service_origin: String,
    pub team_id: String,
    pub skill_id: String,
    pub version_id: String,
    pub sha256: String,
    pub requested_ref: String,
}

impl TeamArtifactManifest {
    pub(crate) fn validate(&self) -> Result<()> {
        let uri: axum::http::Uri = self
            .service_origin
            .parse()
            .context("invalid service origin")?;
        if !matches!(uri.scheme_str(), Some("https" | "http"))
            || uri.authority().is_none()
            || uri.authority().is_some_and(|a| a.as_str().contains('@'))
            || uri.path() != "/"
            || uri.query().is_some()
            || self.service_origin.ends_with('/')
        {
            bail!("service_origin must be a canonical HTTP(S) origin without credentials or path");
        }
        for value in [&self.team_id, &self.skill_id, &self.version_id] {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                bail!("team, skill and version identifiers must be safe nonempty identifiers");
            }
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("sha256 must contain 64 lowercase hexadecimal characters");
        }
        if self.requested_ref.is_empty()
            || self.requested_ref.len() > 128
            || self.requested_ref.chars().any(char::is_control)
        {
            bail!("requested_ref must be a nonempty version selector");
        }
        Ok(())
    }

    pub(crate) fn same_source(&self, other: &Self) -> bool {
        self.service_origin == other.service_origin
            && self.team_id == other.team_id
            && self.skill_id == other.skill_id
    }

    pub(crate) fn descriptor(&self) -> SourceDescriptor {
        SourceDescriptor {
            provider: "team".into(),
            locator: format!(
                "{}/v1/teams/{}/skills/{}",
                self.service_origin, self.team_id, self.skill_id
            ),
            repository: None,
            path: None,
            subdir: String::new(),
            requested_ref: Some(self.requested_ref.clone()),
            resolved_commit: None,
            tree_sha: None,
            team: Some(self.clone()),
            team_tree_digest: None,
        }
    }
}

/// Extract only ordinary files/directories into a newly-created private directory.
/// The compressed bytes are read once, bounded, and hashed before tar parsing.
pub(crate) fn extract(
    archive: &Path,
    manifest: &TeamArtifactManifest,
    destination: &Path,
) -> Result<()> {
    manifest.validate()?;
    let mut bytes = Vec::new();
    fs::File::open(archive)?
        .take(COMPRESSED_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > COMPRESSED_LIMIT {
        bail!("compressed artifact exceeds 10 MiB");
    }
    let mut hash = Sha256::new();
    hash.update(&bytes);
    if to_hex(&hash.finalize()) != manifest.sha256 {
        bail!("artifact SHA-256 mismatch");
    }
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(destination)
        .context("artifact destination must not already exist")?;
    let result = extract_bytes(&bytes, destination);
    if result.is_err() {
        fs::remove_dir_all(destination).context("remove rejected artifact staging")?;
    }
    result
}

fn extract_bytes(bytes: &[u8], destination: &Path) -> Result<()> {
    // Bound tar headers, padding and trailing decompressed bytes as well as file sizes.
    let decoder = GzDecoder::new(bytes);
    let mut expanded = Vec::new();
    decoder
        .take(EXPANDED_LIMIT + 1)
        .read_to_end(&mut expanded)?;
    if expanded.len() as u64 > EXPANDED_LIMIT {
        bail!("expanded artifact exceeds 50 MiB");
    }
    let mut archive = tar::Archive::new(&expanded[..]);
    let mut paths = BTreeSet::new();
    let mut files = 0;
    for item in archive.entries()? {
        let mut entry = item?;
        let path = entry.path()?.into_owned();
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            bail!("artifact contains an absolute or non-normal path");
        }
        for component in path.components() {
            let text = component
                .as_os_str()
                .to_str()
                .context("artifact paths must be UTF-8")?;
            let lower = text.to_ascii_lowercase();
            if lower == ".git"
                || lower == ".env"
                || lower.starts_with(".env.")
                || text.contains('\\')
                || text.contains(':')
                || text.chars().any(char::is_control)
            {
                bail!("artifact contains a private or unsafe path");
            }
        }
        // Case folding avoids platform-dependent duplicate writes on macOS/Windows.
        if !paths.insert(path.to_string_lossy().to_lowercase()) {
            bail!("artifact contains a duplicate path");
        }
        let kind = entry.header().entry_type();
        let target = destination.join(&path);
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
        } else if kind.is_file() {
            files += 1;
            if files > 1000 {
                bail!("artifact contains more than 1000 files");
            }
            fs::create_dir_all(target.parent().context("artifact file has no parent")?)?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)?;
            std::io::copy(&mut entry, &mut output)?;
            output.flush()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = if entry.header().mode()? & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                };
                fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
            }
        } else {
            bail!("artifact links and special files are not allowed");
        }
    }
    if !destination.join("SKILL.md").is_file() {
        bail!("artifact root must contain SKILL.md");
    }
    Ok(())
}

/// Sealed input and metadata images are carried by the existing convergence plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TeamInput {
    pub manifest: TeamArtifactManifest,
    pub input_path: String,
    pub old_sources: Option<String>,
    pub old_lock: Option<String>,
    pub new_sources: String,
    pub new_lock: String,
}

pub(crate) const METADATA_PATHS: [&str; 2] = ["state/registry/sources.json", "loom.lock"];

pub(crate) fn source_digest(path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("absent".into()),
        Err(error) => Err(error.into()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("canonical source must be a concrete directory")
        }
        Ok(_) => super::provenance::skill_tree_digest(path),
    }
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

impl TeamInput {
    pub(crate) fn validate_metadata(&self, root: &Path, committed: bool) -> Result<()> {
        for (rel, old, new) in self.metadata() {
            let current = read_optional(&root.join(rel))?;
            let expected = if committed { Some(new) } else { old };
            if current.as_deref() != expected {
                bail!("team provenance changed outside the reviewed transaction: {rel}");
            }
        }
        Ok(())
    }

    pub(crate) fn write_metadata(&self, root: &Path, restore: bool) -> Result<()> {
        for (rel, old, new) in self.metadata() {
            let path = root.join(rel);
            let current = read_optional(&path)?;
            if current.as_deref() != old && current.as_deref() != Some(new) {
                bail!("team metadata has concurrent edits; preserved {rel}");
            }
            let next = if restore { old } else { Some(new) };
            if current.as_deref() == next {
                continue;
            }
            if let Some(next) = next {
                fs::create_dir_all(path.parent().context("metadata parent missing")?)?;
                crate::fs_util::write_atomic(&path, next)?;
                #[cfg(debug_assertions)]
                if !restore
                    && rel == METADATA_PATHS[0]
                    && std::env::var("LOOM_FAULT_INJECT").ok().as_deref()
                        == Some("convergence_interrupt_after_team_sources")
                {
                    bail!("injected interruption after team sources metadata");
                }
            } else {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }

    fn metadata(&self) -> [(&str, Option<&str>, &str); 2] {
        [
            (
                METADATA_PATHS[0],
                self.old_sources.as_deref(),
                &self.new_sources,
            ),
            (METADATA_PATHS[1], self.old_lock.as_deref(), &self.new_lock),
        ]
    }
}

pub(crate) fn write_candidate_metadata(
    ctx: &crate::state::AppContext,
    team: &crate::commands::team_package::TeamInput,
) -> Result<()> {
    for (rel, value) in crate::commands::team_package::METADATA_PATHS
        .into_iter()
        .zip([&team.new_sources, &team.new_lock])
    {
        let path = ctx.root.join(rel);
        std::fs::create_dir_all(path.parent().expect("metadata parent"))?;
        crate::fs_util::write_atomic(&path, value)?;
    }
    Ok(())
}

/// Own a validated but unpublished candidate until its plan can be handed off.
pub(crate) struct UnpublishedInput {
    path: std::path::PathBuf,
    published: bool,
}
impl UnpublishedInput {
    pub(crate) fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            published: false,
        }
    }
    pub(crate) fn publish(&mut self) {
        self.published = true;
    }
}
impl Drop for UnpublishedInput {
    fn drop(&mut self) {
        if !self.published
            && let Err(error) = fs::remove_dir_all(&self.path)
        {
            eprintln!("failed to clean unpublished team candidate: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package(path: &str, kind: tar::EntryType) -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(1);
        header.set_mode(0o644);
        header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        let mut tar = tar::Builder::new(Vec::new());
        tar.append(&header, &b"x"[..]).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&tar.into_inner().unwrap()).unwrap();
        gzip.finish().unwrap()
    }
    #[test]
    fn rejects_traversal_links_and_private_files() {
        for (path, kind, expected) in [
            ("../SKILL.md", tar::EntryType::Regular, "non-normal"),
            ("/SKILL.md", tar::EntryType::Regular, "non-normal"),
            ("SKILL.md", tar::EntryType::Symlink, "links"),
            ("SKILL.md", tar::EntryType::Link, "links"),
            (".env", tar::EntryType::Regular, "private"),
            (".GIT/config", tar::EntryType::Regular, "private"),
        ] {
            let root = std::env::temp_dir()
                .join(format!("loom-team-archive-test-{}", uuid::Uuid::new_v4()));
            let destination = root.join("extract");
            fs::create_dir_all(&destination).unwrap();
            let result = extract_bytes(&package(path, kind), &destination);
            fs::remove_dir_all(&root).unwrap();
            assert!(
                result.unwrap_err().to_string().contains(expected),
                "wrong rejection for {path}"
            );
        }
    }
    #[test]
    fn bounds_expanded_stream() {
        let root =
            std::env::temp_dir().join(format!("loom-team-size-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&vec![0; EXPANDED_LIMIT as usize + 1])
            .unwrap();
        let result = extract_bytes(&gzip.finish().unwrap(), &root);
        fs::remove_dir_all(&root).unwrap();
        assert!(result.unwrap_err().to_string().contains("50 MiB"));
    }
}
