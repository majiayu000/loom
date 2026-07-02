use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::sha256::{Sha256, to_hex};

pub(super) const PACKAGE_SCHEMA_VERSION: u32 = 1;
pub(super) const SUPPORTED_FORMAT: &str = "agent-skills-archive";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackagePlan {
    pub schema_version: u32,
    pub plan_id: String,
    pub created_at: DateTime<Utc>,
    pub source: PackageSource,
    pub format: String,
    pub loom_version: String,
    pub source_ref: String,
    pub source_digest: String,
    pub files: Vec<PackageFilePlan>,
    pub checks: PackageChecks,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageManifest {
    pub schema_version: u32,
    pub plan_id: String,
    pub created_at: DateTime<Utc>,
    pub source: PackageSource,
    pub format: String,
    pub loom_version: String,
    pub source_ref: String,
    pub source_digest: String,
    pub files: Vec<PackageFilePlan>,
    pub checks: PackageChecks,
    pub build: PackageBuildMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageBuildMetadata {
    pub artifact_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageSource {
    pub kind: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub members: Vec<PackageSourceMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageSourceMember {
    pub skill_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageFilePlan {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PackageChecks {
    pub portable_lint: String,
    pub safety_scan: String,
    pub eval_gate: String,
    pub approval: String,
}

#[derive(Debug, Clone)]
pub(super) struct CopyFile {
    pub archive_rel: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

pub(super) struct PackageTempDir {
    pub path: PathBuf,
}

impl PackageTempDir {
    pub fn new(prefix: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn new_in(parent: &std::path::Path, prefix: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(parent)?;
        let path = parent.join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for PackageTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(super) fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}
