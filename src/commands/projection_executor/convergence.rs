use std::path::PathBuf;

use serde_json::{Value, json};

use crate::fs_util::{exchange_paths_atomic, remove_path_if_exists, rename_no_replace_atomic};
use crate::state::AppContext;
use crate::state_model::RegistryProjectionInstance;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::super::projections::{apply_projection_observation, observe_projection};
use super::super::skill_cmds::shared::push_rollback_error;

#[must_use = "a prepared projection must be activated or explicitly discarded"]
pub(crate) struct PreparedProjection {
    projection: Option<RegistryProjectionInstance>,
    staging_path: Option<PathBuf>,
    materialized_path: Option<PathBuf>,
    pub(super) path_exists: bool,
}

impl PreparedProjection {
    pub(super) fn new(
        projection: RegistryProjectionInstance,
        staging_path: PathBuf,
        materialized_path: PathBuf,
        path_exists: bool,
    ) -> Self {
        Self {
            projection: Some(projection),
            staging_path: Some(staging_path),
            materialized_path: Some(materialized_path),
            path_exists,
        }
    }

    #[cfg(test)]
    pub(super) fn staging_path(&self) -> &std::path::Path {
        self.staging_path
            .as_deref()
            .expect("prepared projection must own a staging path")
    }

    fn take_parts(&mut self) -> (RegistryProjectionInstance, PathBuf, PathBuf, bool) {
        (
            self.projection
                .take()
                .expect("prepared projection must own its projection identity"),
            self.staging_path
                .take()
                .expect("prepared projection must own a staging path"),
            self.materialized_path
                .take()
                .expect("prepared projection must own a materialized path"),
            self.path_exists,
        )
    }
}

impl Drop for PreparedProjection {
    fn drop(&mut self) {
        if let Some(staging_path) = self.staging_path.take()
            && let Err(err) = remove_path_if_exists(&staging_path)
        {
            eprintln!(
                "loom: failed to clean abandoned prepared projection '{}': {err}",
                staging_path.display()
            );
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ProjectionRollbackArtifact {
    Exchanged {
        materialized_path: PathBuf,
        backup_path: PathBuf,
    },
    Created {
        materialized_path: PathBuf,
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
            } => json!({
                "reason": "convergence.atomic_exchange",
                "kind": "atomic_exchange",
                "original_path": materialized_path.display().to_string(),
                "backup_path": backup_path.display().to_string(),
            }),
            Self::Created { materialized_path } => json!({
                "reason": "convergence.atomic_create",
                "kind": "atomic_create",
                "original_path": materialized_path.display().to_string(),
            }),
        }
    }

    fn rollback(self) -> Vec<Value> {
        let mut errors = Vec::new();
        match self {
            Self::Exchanged {
                materialized_path,
                backup_path,
            } => {
                if let Err(err) = exchange_paths_atomic(&backup_path, &materialized_path) {
                    push_rollback_error(&mut errors, "restore_projection_atomic_exchange", err);
                    return errors;
                }
                if let Err(err) = remove_path_if_exists(&backup_path) {
                    push_rollback_error(&mut errors, "remove_projection_staging", err);
                }
            }
            Self::Created { materialized_path } => {
                if let Err(err) = remove_path_if_exists(&materialized_path) {
                    push_rollback_error(&mut errors, "remove_projection_path", err);
                }
            }
        }
        errors
    }

    fn finalize(self) -> std::result::Result<(), CommandFailure> {
        if let Self::Exchanged { backup_path, .. } = self {
            remove_path_if_exists(&backup_path).map_err(map_io)?;
        }
        Ok(())
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
    pub(crate) fn rollback(mut self) -> std::result::Result<(), CommandFailure> {
        let projection = self
            .projection
            .take()
            .expect("activated projection must own its projection identity");
        let errors = self
            .rollback_artifact
            .take()
            .expect("activated projection must own a rollback artifact")
            .rollback();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CommandFailure::new(
                ErrorCode::InternalError,
                format!(
                    "failed to rollback convergence projection '{}'",
                    projection.instance_id
                ),
            )
            .with_rollback_errors(errors))
        }
    }

    #[allow(
        dead_code,
        reason = "consumed by the SP524-T004 convergence transaction"
    )]
    pub(crate) fn finalize(
        mut self,
    ) -> std::result::Result<RegistryProjectionInstance, CommandFailure> {
        self.rollback_artifact
            .take()
            .expect("activated projection must own a rollback artifact")
            .finalize()?;
        Ok(self
            .projection
            .take()
            .expect("activated projection must own its projection identity"))
    }
}

impl Drop for ProjectionActivationOutput {
    fn drop(&mut self) {
        if let Some(artifact) = self.rollback_artifact.take() {
            for error in artifact.rollback() {
                eprintln!("loom: failed to rollback abandoned projection activation: {error}");
            }
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
    let (mut projection, staging_path, materialized_path, path_exists) = prepared.take_parts();

    let rollback_artifact = if path_exists {
        if let Err(err) = exchange_paths_atomic(&staging_path, &materialized_path) {
            let mut cleanup_errors = Vec::new();
            cleanup_staging(&staging_path, &mut cleanup_errors);
            return Err(map_atomic_operation_error(err, &materialized_path)
                .with_rollback_errors(cleanup_errors));
        }
        ProjectionRollbackArtifact::Exchanged {
            materialized_path,
            backup_path: staging_path,
        }
    } else {
        if let Err(err) = rename_no_replace_atomic(&staging_path, &materialized_path) {
            let mut cleanup_errors = Vec::new();
            cleanup_staging(&staging_path, &mut cleanup_errors);
            return Err(map_atomic_operation_error(err, &materialized_path)
                .with_rollback_errors(cleanup_errors));
        }
        ProjectionRollbackArtifact::Created { materialized_path }
    };

    let observation = observe_projection(ctx, &projection);
    if observation.status != "healthy" {
        let rollback_errors = rollback_artifact.rollback();
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
    let staging_path = prepared
        .staging_path
        .take()
        .expect("prepared projection must own a staging path");
    remove_path_if_exists(&staging_path).map_err(map_io)
}

fn cleanup_staging(path: &std::path::Path, errors: &mut Vec<Value>) {
    if let Err(err) = remove_path_if_exists(path) {
        push_rollback_error(errors, "remove_projection_staging", err);
    }
}

fn map_atomic_operation_error(err: std::io::Error, path: &std::path::Path) -> CommandFailure {
    match err.kind() {
        std::io::ErrorKind::AlreadyExists => CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "projection path '{}' appeared after convergence preparation; concurrent entry was preserved",
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
