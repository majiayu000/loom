use std::path::{Path, PathBuf};
use std::{fs, io};

use serde_json::json;

use crate::fs_util::{remove_path_if_exists, rename_no_replace_atomic};
use crate::state_model::RegistryProjectionInstance;
use crate::types::ErrorCode;

use super::super::{
    CommandFailure,
    helpers::map_io,
    projections::{ProjectionObservation, observe_projection_from_source},
    skill_cmds::shared::push_rollback_error,
};
use super::ProjectionExecutionInput;
use super::convergence::{map_ownership_fingerprint_error, projection_ownership_fingerprint};
#[cfg(test)]
use super::{ProjectionExecutionContext, ProjectionMethod};

pub(super) fn observe_projection_path(
    projection: &RegistryProjectionInstance,
    source_path: &Path,
    path: &Path,
) -> ProjectionObservation {
    let mut staged = projection.clone();
    staged.materialized_path = path.display().to_string();
    observe_projection_from_source(&staged, source_path)
}

#[cfg(test)]
pub(super) fn inject_convergence_staging_mismatch(
    input: &ProjectionExecutionInput,
    staging_path: &Path,
) -> Result<(), CommandFailure> {
    if input.context == ProjectionExecutionContext::Convergence
        && matches!(
            input.after_materialize_fault,
            Some(
                "test_convergence_staging_mismatch"
                    | "test_convergence_observation_failure_replacement"
            )
        )
        && !matches!(input.method, ProjectionMethod::Symlink)
    {
        if input.after_materialize_fault == Some("test_convergence_observation_failure_replacement")
        {
            remove_path_if_exists(staging_path).map_err(map_io)?;
            std::fs::create_dir(staging_path).map_err(map_io)?;
            std::fs::write(staging_path.join("external.txt"), "external\n").map_err(map_io)?;
        } else {
            std::fs::write(staging_path.join("details.txt"), "fault-injected drift\n")
                .map_err(map_io)?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn inject_convergence_project_failure_replacement(
    input: &ProjectionExecutionInput,
    staging_path: &Path,
) -> Result<(), CommandFailure> {
    if input.after_materialize_fault == Some("test_convergence_project_failure_replacement") {
        std::fs::create_dir(staging_path).map_err(map_io)?;
        std::fs::write(staging_path.join("external.txt"), "external\n").map_err(map_io)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn inject_convergence_anchor_failure(
    input: &ProjectionExecutionInput,
    staging_path: &Path,
) -> Result<(), CommandFailure> {
    if input.after_materialize_fault == Some("test_convergence_fingerprint_failure_replacement") {
        remove_path_if_exists(staging_path).map_err(map_io)?;
        std::fs::create_dir(staging_path).map_err(map_io)?;
        std::fs::write(staging_path.join("external.txt"), "external\n").map_err(map_io)?;
        return Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            "injected staging fingerprint failure",
        ));
    }
    Ok(())
}

#[cfg(not(test))]
pub(super) fn inject_convergence_staging_mismatch(
    _input: &ProjectionExecutionInput,
    _staging_path: &Path,
) -> Result<(), CommandFailure> {
    Ok(())
}

#[cfg(not(test))]
pub(super) fn inject_convergence_project_failure_replacement(
    _input: &ProjectionExecutionInput,
    _staging_path: &Path,
) -> Result<(), CommandFailure> {
    Ok(())
}

#[cfg(not(test))]
pub(super) fn inject_convergence_anchor_failure(
    _input: &ProjectionExecutionInput,
    _staging_path: &Path,
) -> Result<(), CommandFailure> {
    Ok(())
}

pub(super) struct StagingOwnership {
    path: PathBuf,
    digest: String,
}

pub(super) enum CleanupClaim {
    Missing,
    Claimed(PathBuf),
}

pub(super) fn path_entry_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) fn claim_for_cleanup(path: &Path, suffix: &str) -> io::Result<CleanupClaim> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "artifact has no file name"))?;
    let claim_path = path.with_file_name(format!("{}{suffix}", name.to_string_lossy()));
    let original_exists = path_entry_exists(path)?;
    let claim_exists = path_entry_exists(&claim_path)?;
    if original_exists && claim_exists {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "artifact and cleanup claim both exist",
        ));
    }
    if claim_exists {
        return Ok(CleanupClaim::Claimed(claim_path));
    }
    if !original_exists {
        return Ok(CleanupClaim::Missing);
    }
    rename_no_replace_atomic(path, &claim_path)?;
    Ok(CleanupClaim::Claimed(claim_path))
}

impl StagingOwnership {
    pub(super) fn new(path: PathBuf, digest: String) -> Self {
        Self { path, digest }
    }

    pub(super) fn digest(&self) -> &str {
        &self.digest
    }

    pub(super) fn claim_and_retain(self) -> Result<(), CommandFailure> {
        self.claim_and_retain_with_after_validation(|_| Ok(()))
    }

    fn claim_and_retain_with_after_validation(
        self,
        after_validation: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), CommandFailure> {
        let claim_path = match claim_for_cleanup(&self.path, ".staging-cleanup-claim")
            .map_err(map_io)
            .map_err(|err| attach_recovery(err, &self.path, None, &self.digest))?
        {
            CleanupClaim::Claimed(path) => path,
            CleanupClaim::Missing => {
                return Err(recovery_failure(
                    ErrorCode::ProjectionConflict,
                    "anchored staging disappeared before cleanup",
                    &self.path,
                    None,
                    &self.digest,
                ));
            }
        };
        let actual = projection_ownership_fingerprint(&claim_path)
            .map_err(|err| map_ownership_fingerprint_error(err, &claim_path))
            .map_err(|err| attach_recovery(err, &self.path, Some(&claim_path), &self.digest))?;
        if actual != self.digest {
            return Err(recovery_failure(
                ErrorCode::ProjectionConflict,
                "claimed staging changed; concurrent data was preserved",
                &self.path,
                Some(&claim_path),
                &self.digest,
            ));
        }
        after_validation(&claim_path)
            .map_err(map_io)
            .map_err(|err| attach_recovery(err, &self.path, Some(&claim_path), &self.digest))?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn claim_and_retain_after_validation(
        self,
        after_validation: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), CommandFailure> {
        self.claim_and_retain_with_after_validation(after_validation)
    }
}

pub(super) fn cleanup_owned_staging(ownership: Option<StagingOwnership>) -> Vec<serde_json::Value> {
    let Some(ownership) = ownership else {
        return vec![json!({
            "step": "claim_projection_staging",
            "recovery_required": true,
            "reason": "ownership_anchor_missing",
        })];
    };
    match ownership.claim_and_retain() {
        Ok(()) => Vec::new(),
        Err(err) => vec![json!({
            "step": "claim_projection_staging",
            "code": err.code.as_str(),
            "message": err.message,
            "details": err.details,
        })],
    }
}

pub(super) fn cleanup_projection_staging(path: &Path, errors: &mut Vec<serde_json::Value>) {
    if let Err(err) = remove_path_if_exists(path) {
        push_rollback_error(errors, "remove_projection_staging", err);
    }
}

pub(super) fn preserve_unverified_staging(
    failure: CommandFailure,
    staging_path: &Path,
) -> CommandFailure {
    if std::fs::symlink_metadata(staging_path).is_err() {
        return failure;
    }
    failure.with_rollback_errors(vec![json!({
        "step": "preserve_unverified_projection_staging",
        "recovery_required": true,
        "staging_path": staging_path.display().to_string(),
        "reason": "ownership_not_anchored",
    })])
}

fn attach_recovery(
    mut failure: CommandFailure,
    staging_path: &Path,
    claim_path: Option<&Path>,
    expected_digest: &str,
) -> CommandFailure {
    let cause = std::mem::replace(&mut failure.details, json!({}));
    failure.details = json!({
        "recovery_required": true,
        "staging_path": staging_path.display().to_string(),
        "claim_path": claim_path.map(|path| path.display().to_string()),
        "expected_digest": expected_digest,
        "cause": cause,
    });
    failure
}

fn recovery_failure(
    code: ErrorCode,
    message: &str,
    staging_path: &Path,
    claim_path: Option<&Path>,
    expected_digest: &str,
) -> CommandFailure {
    attach_recovery(
        CommandFailure::new(code, message),
        staging_path,
        claim_path,
        expected_digest,
    )
}
