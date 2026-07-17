use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::fs_util::{exchange_paths_atomic, remove_path_if_exists, rename_no_replace_atomic};
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::RegistryProjectionInstance;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::super::projections::{apply_projection_observation, observe_projection};
use super::super::skill_cmds::shared::push_rollback_error;

struct PreparedProjectionParts {
    projection: RegistryProjectionInstance,
    staging_path: PathBuf,
    materialized_path: PathBuf,
    path_exists: bool,
    staging_digest: String,
    existing_digest: Option<String>,
}

#[must_use = "a prepared projection must be activated or explicitly discarded"]
pub(crate) struct PreparedProjection {
    parts: Option<PreparedProjectionParts>,
}

impl PreparedProjection {
    pub(super) fn new(
        projection: RegistryProjectionInstance,
        staging_path: PathBuf,
        materialized_path: PathBuf,
        path_exists: bool,
        staging_digest: String,
        existing_digest: Option<String>,
    ) -> Self {
        Self {
            parts: Some(PreparedProjectionParts {
                projection,
                staging_path,
                materialized_path,
                path_exists,
                staging_digest,
                existing_digest,
            }),
        }
    }

    #[cfg(test)]
    pub(super) fn staging_path(&self) -> &Path {
        &self
            .parts
            .as_ref()
            .expect("prepared projection must own its transaction state")
            .staging_path
    }

    fn take_parts(&mut self) -> PreparedProjectionParts {
        self.parts
            .take()
            .expect("prepared projection must own its transaction state")
    }
}

impl Drop for PreparedProjection {
    fn drop(&mut self) {
        if let Some(parts) = self.parts.take()
            && let Err(err) = remove_path_if_exists(&parts.staging_path)
        {
            eprintln!(
                "loom: failed to clean abandoned prepared projection '{}': {err}",
                parts.staging_path.display()
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingCleanupReason {
    RollbackExchanged,
    RollbackCreated,
}

impl PendingCleanupReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::RollbackExchanged => "rollback_exchanged",
            Self::RollbackCreated => "rollback_created",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProjectionRollbackArtifact {
    Exchanged {
        materialized_path: PathBuf,
        backup_path: PathBuf,
        activated_digest: String,
        original_digest: String,
    },
    Created {
        materialized_path: PathBuf,
        rollback_path: PathBuf,
        activated_digest: String,
    },
    PendingCleanup {
        materialized_path: PathBuf,
        artifact_path: PathBuf,
        expected_digest: String,
        reason: PendingCleanupReason,
    },
}

impl ProjectionRollbackArtifact {
    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn evidence(&self) -> Value {
        match self {
            Self::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                original_digest,
            } => json!({
                "reason": "convergence.atomic_exchange",
                "kind": "atomic_exchange",
                "original_path": materialized_path.display().to_string(),
                "backup_path": backup_path.display().to_string(),
                "activated_digest": activated_digest,
                "original_digest": original_digest,
            }),
            Self::Created {
                materialized_path,
                rollback_path,
                activated_digest,
            } => json!({
                "reason": "convergence.atomic_create",
                "kind": "atomic_create",
                "original_path": materialized_path.display().to_string(),
                "rollback_path": rollback_path.display().to_string(),
                "activated_digest": activated_digest,
            }),
            Self::PendingCleanup {
                materialized_path,
                artifact_path,
                expected_digest,
                reason,
            } => json!({
                "reason": reason.as_str(),
                "kind": "pending_cleanup",
                "original_path": materialized_path.display().to_string(),
                "artifact_path": artifact_path.display().to_string(),
                "expected_digest": expected_digest,
            }),
        }
    }

    fn rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        match self.clone() {
            Self::Exchanged {
                materialized_path,
                backup_path,
                activated_digest,
                ..
            } => {
                validate_owned_digest(
                    &materialized_path,
                    &activated_digest,
                    "rollback live projection",
                    self,
                )?;
                exchange_paths_atomic(&backup_path, &materialized_path)
                    .map_err(map_atomic_exchange_error)
                    .map_err(|err| with_recovery_details(err, self))?;
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: backup_path,
                    expected_digest: activated_digest,
                    reason: PendingCleanupReason::RollbackExchanged,
                };
                self.cleanup_pending()
            }
            Self::Created {
                materialized_path,
                rollback_path,
                activated_digest,
            } => {
                validate_owned_digest(
                    &materialized_path,
                    &activated_digest,
                    "rollback created projection",
                    self,
                )?;
                rename_no_replace_atomic(&materialized_path, &rollback_path)
                    .map_err(|err| map_atomic_operation_error(err, &rollback_path))
                    .map_err(|err| with_recovery_details(err, self))?;
                *self = Self::PendingCleanup {
                    materialized_path,
                    artifact_path: rollback_path,
                    expected_digest: activated_digest,
                    reason: PendingCleanupReason::RollbackCreated,
                };
                self.cleanup_pending()
            }
            Self::PendingCleanup { .. } => self.cleanup_pending(),
        }
    }

    fn finalize(&mut self) -> std::result::Result<(), CommandFailure> {
        match self.clone() {
            Self::Exchanged {
                backup_path,
                original_digest,
                ..
            } => {
                validate_owned_digest(
                    &backup_path,
                    &original_digest,
                    "finalize projection backup",
                    self,
                )?;
                remove_path_if_exists(&backup_path)
                    .map_err(map_io)
                    .map_err(|err| with_recovery_details(err, self))
            }
            Self::Created { .. } => Ok(()),
            Self::PendingCleanup { .. } => self.cleanup_pending(),
        }
    }

    fn cleanup_pending(&mut self) -> std::result::Result<(), CommandFailure> {
        let (artifact_path, expected_digest) = match self {
            Self::PendingCleanup {
                artifact_path,
                expected_digest,
                ..
            } => (artifact_path.clone(), expected_digest.clone()),
            _ => {
                return Err(CommandFailure::new(
                    ErrorCode::InternalError,
                    "projection rollback artifact is not pending cleanup",
                ));
            }
        };
        validate_owned_digest(
            &artifact_path,
            &expected_digest,
            "clean projection rollback artifact",
            self,
        )?;
        remove_path_if_exists(&artifact_path)
            .map_err(map_io)
            .map_err(|err| with_recovery_details(err, self))
    }
}

#[must_use = "an activated projection must be finalized or rolled back"]
pub(crate) struct ProjectionActivationOutput {
    projection: Option<RegistryProjectionInstance>,
    rollback_artifact: Option<ProjectionRollbackArtifact>,
}

impl ProjectionActivationOutput {
    #[cfg(test)]
    pub(crate) fn projection(&self) -> &RegistryProjectionInstance {
        self.projection
            .as_ref()
            .expect("activated projection must own its projection identity")
    }

    #[cfg(test)]
    pub(crate) fn rollback_evidence(&self) -> Value {
        self.rollback_artifact
            .as_ref()
            .expect("activated projection must own a rollback artifact")
            .evidence()
    }

    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn rollback(&mut self) -> std::result::Result<(), CommandFailure> {
        let artifact = self
            .rollback_artifact
            .as_mut()
            .expect("activated projection must own a rollback artifact");
        artifact.rollback()?;
        self.rollback_artifact = None;
        self.projection = None;
        Ok(())
    }

    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn finalize(
        &mut self,
    ) -> std::result::Result<RegistryProjectionInstance, CommandFailure> {
        let artifact = self
            .rollback_artifact
            .as_mut()
            .expect("activated projection must own a rollback artifact");
        artifact.finalize()?;
        self.rollback_artifact = None;
        Ok(self
            .projection
            .take()
            .expect("activated projection must own its projection identity"))
    }
}

impl Drop for ProjectionActivationOutput {
    fn drop(&mut self) {
        if let Some(artifact) = self.rollback_artifact.as_mut()
            && let Err(err) = artifact.rollback()
        {
            eprintln!(
                "loom: abandoned projection activation requires recovery: {}; details={}",
                err.message, err.details
            );
        }
    }
}

#[allow(
    dead_code,
    reason = "consumed by the SP524-T004 convergence transaction"
)]
pub(crate) fn activate_prepared_projection(
    ctx: &AppContext,
    prepared: PreparedProjection,
) -> std::result::Result<ProjectionActivationOutput, CommandFailure> {
    let mut prepared = prepared;
    let parts = prepared.take_parts();
    validate_before_activation(&parts).map_err(|err| cleanup_prepare_failure(err, &parts))?;

    let mut rollback_artifact = if parts.path_exists {
        if let Err(err) = exchange_paths_atomic(&parts.staging_path, &parts.materialized_path) {
            return Err(cleanup_prepare_failure(
                map_atomic_exchange_error(err),
                &parts,
            ));
        }
        ProjectionRollbackArtifact::Exchanged {
            materialized_path: parts.materialized_path,
            backup_path: parts.staging_path,
            activated_digest: parts.staging_digest,
            original_digest: parts
                .existing_digest
                .expect("existing convergence path must have a prepared digest"),
        }
    } else {
        if let Err(err) = rename_no_replace_atomic(&parts.staging_path, &parts.materialized_path) {
            return Err(cleanup_prepare_failure(
                map_atomic_operation_error(err, &parts.materialized_path),
                &parts,
            ));
        }
        ProjectionRollbackArtifact::Created {
            materialized_path: parts.materialized_path,
            rollback_path: parts.staging_path,
            activated_digest: parts.staging_digest,
        }
    };

    let mut projection = parts.projection;
    let observation = observe_projection(ctx, &projection);
    if observation.status != "healthy" {
        let mut rollback_errors = Vec::new();
        if let Err(err) = rollback_artifact.rollback() {
            rollback_errors.push(json!({
                "step": "rollback_projection_activation",
                "code": err.code.as_str(),
                "message": err.message,
                "details": err.details,
                "artifact": rollback_artifact.evidence(),
            }));
        }
        return Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "convergence projection '{}' failed post-activation validation: {}",
                projection.instance_id,
                observation
                    .error_code
                    .unwrap_or("projection_validation_failed")
            ),
        )
        .with_rollback_errors(rollback_errors));
    }
    apply_projection_observation(&mut projection, &observation);
    Ok(ProjectionActivationOutput {
        projection: Some(projection),
        rollback_artifact: Some(rollback_artifact),
    })
}

#[allow(
    dead_code,
    reason = "consumed by the SP524-T004 convergence transaction"
)]
pub(crate) fn discard_prepared_projection(
    mut prepared: PreparedProjection,
) -> std::result::Result<(), CommandFailure> {
    let parts = prepared.take_parts();
    remove_path_if_exists(&parts.staging_path).map_err(map_io)
}

fn validate_before_activation(
    parts: &PreparedProjectionParts,
) -> std::result::Result<(), CommandFailure> {
    validate_prepared_digest(
        &parts.staging_path,
        &parts.staging_digest,
        "staging projection",
    )?;
    if let Some(existing_digest) = &parts.existing_digest {
        validate_prepared_digest(
            &parts.materialized_path,
            existing_digest,
            "existing live projection",
        )?;
    }
    Ok(())
}

fn validate_prepared_digest(
    path: &Path,
    expected_digest: &str,
    label: &str,
) -> std::result::Result<(), CommandFailure> {
    let actual_digest = projection_ownership_fingerprint(path).map_err(|err| {
        CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!("failed to validate {label} '{}': {err}", path.display()),
        )
    })?;
    if actual_digest == expected_digest {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!("{label} '{}' changed after preparation", path.display()),
    );
    failure.details = json!({
        "path": path.display().to_string(),
        "expected_digest": expected_digest,
        "actual_digest": actual_digest,
    });
    Err(failure)
}

fn validate_owned_digest(
    path: &Path,
    expected_digest: &str,
    operation: &str,
    artifact: &ProjectionRollbackArtifact,
) -> std::result::Result<(), CommandFailure> {
    let actual_digest = projection_ownership_fingerprint(path).map_err(|err| {
        with_recovery_details(
            CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!(
                    "cannot {operation} because '{}' is unavailable or unreadable: {err}",
                    path.display()
                ),
            ),
            artifact,
        )
    })?;
    if actual_digest == expected_digest {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!(
            "cannot {operation} because '{}' changed after activation; concurrent data was preserved",
            path.display()
        ),
    );
    failure.details = json!({
        "path": path.display().to_string(),
        "expected_digest": expected_digest,
        "actual_digest": actual_digest,
    });
    Err(with_recovery_details(failure, artifact))
}

fn cleanup_prepare_failure(err: CommandFailure, parts: &PreparedProjectionParts) -> CommandFailure {
    let mut cleanup_errors = Vec::new();
    if let Err(cleanup_err) = remove_path_if_exists(&parts.staging_path) {
        push_rollback_error(
            &mut cleanup_errors,
            "remove_projection_staging",
            cleanup_err,
        );
    }
    err.with_rollback_errors(cleanup_errors)
}

pub(super) fn projection_ownership_fingerprint(path: &Path) -> anyhow::Result<String> {
    let mut entries = WalkDir::new(path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.with_context(|| format!("walk {}", path.display())))
        .collect::<anyhow::Result<Vec<_>>>()?;
    entries.sort_by(|left, right| left.path().cmp(right.path()));

    let mut hasher = Sha256::new();
    hasher.update(b"loom-projection-ownership-v1\0");
    for entry in entries {
        let full = entry.path();
        let relative = full
            .strip_prefix(path)
            .with_context(|| format!("strip {}", path.display()))?;
        hash_os_str(&mut hasher, relative.as_os_str());

        let metadata =
            fs::symlink_metadata(full).with_context(|| format!("stat {}", full.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            hasher.update(b"directory\0");
        } else if file_type.is_symlink() {
            hasher.update(b"symlink\0");
            hash_os_str(
                &mut hasher,
                fs::read_link(full)
                    .with_context(|| format!("readlink {}", full.display()))?
                    .as_os_str(),
            );
        } else if file_type.is_file() {
            hasher.update(b"file\0");
            let mut file =
                fs::File::open(full).with_context(|| format!("open {}", full.display()))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .with_context(|| format!("read {}", full.display()))?;
            hasher.update(&(bytes.len() as u64).to_be_bytes());
            hasher.update(&bytes);
        } else {
            hasher.update(b"special\0");
        }
        hash_ownership_metadata(&mut hasher, full, &metadata, file_type.is_file())?;
        hasher.update(b"entry-end\0");
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

#[cfg(unix)]
fn hash_os_str(hasher: &mut Sha256, value: &OsStr) {
    use std::os::unix::ffi::OsStrExt;

    let bytes = value.as_bytes();
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(windows)]
fn hash_os_str(hasher: &mut Sha256, value: &OsStr) {
    use std::os::windows::ffi::OsStrExt;

    let words = value.encode_wide().collect::<Vec<_>>();
    hasher.update(&(words.len() as u64).to_be_bytes());
    for word in words {
        hasher.update(&word.to_be_bytes());
    }
}

#[cfg(unix)]
fn hash_ownership_metadata(
    hasher: &mut Sha256,
    path: &Path,
    metadata: &fs::Metadata,
    include_write_time: bool,
) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    for value in [
        metadata.dev(),
        metadata.ino(),
        u64::from(metadata.mode()),
        metadata.nlink(),
        u64::from(metadata.uid()),
        u64::from(metadata.gid()),
        metadata.rdev(),
        metadata.size(),
    ] {
        hasher.update(&value.to_be_bytes());
    }
    if include_write_time {
        hasher.update(&metadata.mtime().to_be_bytes());
        hasher.update(&metadata.mtime_nsec().to_be_bytes());
    }
    let mut names = xattr::list(path)
        .with_context(|| format!("list xattrs {}", path.display()))?
        .collect::<Vec<_>>();
    names.sort();
    for name in names {
        hasher.update(b"xattr\0");
        hash_os_str(hasher, &name);
        let value = xattr::get(path, &name)
            .with_context(|| format!("read xattr {:?} on {}", name, path.display()))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "xattr {:?} disappeared while fingerprinting {}",
                    name,
                    path.display()
                )
            })?;
        hasher.update(&(value.len() as u64).to_be_bytes());
        hasher.update(&value);
    }
    Ok(())
}

#[cfg(windows)]
fn hash_ownership_metadata(
    hasher: &mut Sha256,
    path: &Path,
    metadata: &fs::Metadata,
    include_write_time: bool,
) -> anyhow::Result<()> {
    use std::os::windows::fs::MetadataExt;

    for value in [
        u64::from(metadata.file_attributes()),
        metadata.creation_time(),
        metadata.file_size(),
    ] {
        hasher.update(&value.to_be_bytes());
    }
    let (volume_serial, file_id) = windows_file_identity(path)?;
    hasher.update(&volume_serial.to_be_bytes());
    hasher.update(&file_id);
    if include_write_time {
        hasher.update(&metadata.last_write_time().to_be_bytes());
    }
    Ok(())
}

#[cfg(windows)]
fn windows_file_identity(path: &Path) -> anyhow::Result<(u64, [u8; 16])> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FileIdInfo,
        GetFileInformationByHandleEx,
    };

    let file = OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("open identity handle {}", path.display()))?;
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: the handle stays open for the call, and the output pointer and
    // byte length describe a live `FILE_ID_INFO` value.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("read file identity {}", path.display()));
    }
    Ok((identity.VolumeSerialNumber, identity.FileId.Identifier))
}

fn with_recovery_details(
    mut failure: CommandFailure,
    artifact: &ProjectionRollbackArtifact,
) -> CommandFailure {
    let cause_details = std::mem::replace(&mut failure.details, json!({}));
    failure.details = json!({
        "recovery_required": true,
        "artifact": artifact.evidence(),
        "cause_details": cause_details,
    });
    failure
}

fn map_atomic_exchange_error(err: std::io::Error) -> CommandFailure {
    if err.kind() == std::io::ErrorKind::Unsupported {
        CommandFailure::new(
            ErrorCode::ProjectionMethodUnsupported,
            format!(
                "atomic projection exchange is unavailable on this platform or filesystem: {err}"
            ),
        )
    } else {
        map_io(err)
    }
}

fn map_atomic_operation_error(err: std::io::Error, path: &Path) -> CommandFailure {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "projection path '{}' appeared during atomic activation; concurrent entry was preserved",
                path.display()
            ),
        ),
        std::io::ErrorKind::Unsupported => CommandFailure::new(
            ErrorCode::ProjectionMethodUnsupported,
            format!(
                "atomic projection activation is unavailable on this platform or filesystem: {err}"
            ),
        ),
        _ => map_io(err),
    }
}
