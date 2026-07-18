use std::fs;
use std::path::{Path, PathBuf};

use crate::state::AppContext;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::{CommandFailure, helpers::map_project_io, projections::project_skill_to_target};
#[cfg(test)]
use super::StandaloneMode;
use super::convergence::{map_ownership_fingerprint_error, projection_ownership_fingerprint};
use super::{
    ConvergenceMode, ProjectionExecutionContext, ProjectionExecutionInput,
    ProjectionExecutionOutput, activate_prepared_projection, execute_projection_mode,
    validate_execution_input,
};

pub(crate) fn convergence_projection_fingerprint(path: &Path) -> Result<String, CommandFailure> {
    projection_ownership_fingerprint(path)
        .map_err(|error| map_ownership_fingerprint_error(error, path))
}

#[cfg(test)]
pub(crate) fn execute_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> Result<ProjectionExecutionOutput, CommandFailure> {
    match input.context {
        ProjectionExecutionContext::Standalone => execute_projection_mode::<StandaloneMode>(
            ctx, paths, snapshot, input, None,
        )
        .map(|output| ProjectionExecutionOutput {
            projection: output.projection,
            prepared: None,
            backup: output.backup,
            commit: output.commit,
            meta: output.meta,
            noop: output.noop,
        }),
        ProjectionExecutionContext::Convergence => {
            execute_projection_mode::<ConvergenceMode>(ctx, paths, snapshot, input, None)
        }
    }
}

pub(crate) fn prepare_convergence_projection(
    ctx: &AppContext,
    input: &ProjectionExecutionInput,
    source: &Path,
    staging_path: &Path,
) -> Result<(), CommandFailure> {
    validate_execution_input(ctx, input)?;
    if input.context != ProjectionExecutionContext::Convergence {
        return Err(CommandFailure::new(
            ErrorCode::InternalError,
            "projection preparation requires convergence context",
        ));
    }
    if fs::symlink_metadata(staging_path).is_ok() {
        return Err(CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!(
                "declared projection staging path already exists: {}",
                staging_path.display()
            ),
        ));
    }
    project_skill_to_target(source, staging_path, input.method)
        .map_err(map_project_io(input.method))
}

pub(crate) fn execute_prepared_convergence_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
    staging_path: PathBuf,
) -> Result<ProjectionExecutionOutput, CommandFailure> {
    let mut output = execute_projection_mode::<ConvergenceMode>(
        ctx,
        paths,
        snapshot,
        input,
        Some(staging_path),
    )?;
    let Some(prepared) = output.prepared.take() else {
        return Ok(output);
    };
    let mut activated = activate_prepared_projection(ctx, prepared)?;
    output.projection = Some(activated.finalize()?);
    Ok(output)
}

#[allow(dead_code, reason = "consumed by the SP524-T004 transaction")]
pub(crate) fn execute_convergence_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> Result<ProjectionExecutionOutput, CommandFailure> {
    debug_assert_eq!(input.context, ProjectionExecutionContext::Convergence);
    execute_projection_mode::<ConvergenceMode>(ctx, paths, snapshot, input, None)
}
