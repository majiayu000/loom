use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::state::AppContext;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::{
    CommandFailure,
    helpers::{ensure_skill_exists, map_project_io, validate_projection_method},
    projections::project_skill_to_target,
    skill_safety::enforce_skill_safety,
};
use super::convergence::{map_ownership_fingerprint_error, projection_ownership_fingerprint};
use super::{
    ConvergenceMode, ProjectionExecutionContext, ProjectionExecutionInput,
    ProjectionExecutionOutput, StandaloneMode, StandaloneProjectionExecutionOutput,
    activate_prepared_projection, execute_projection_mode,
};

pub(super) fn validate_execution_input(
    ctx: &AppContext,
    input: &ProjectionExecutionInput,
) -> Result<(), CommandFailure> {
    ensure_skill_exists(ctx, &input.skill)?;
    validate_projection_route(input)?;
    enforce_skill_safety(ctx, &input.skill, &input.binding.policy_profile)?;
    Ok(())
}

fn validate_projection_route(input: &ProjectionExecutionInput) -> Result<(), CommandFailure> {
    if input.target.agent != input.binding.agent {
        return Err(CommandFailure::new(
            ErrorCode::TargetAgentMismatch,
            format!(
                "binding '{}' is for agent '{}' but target '{}' is for agent '{}'",
                input.binding.binding_id,
                input.binding.agent,
                input.target.target_id,
                input.target.agent
            ),
        ));
    }
    if input.target.ownership != crate::core::vocab::Ownership::Managed {
        return Err(CommandFailure::new(
            ErrorCode::TargetNotManaged,
            format!(
                "target '{}' has ownership '{}' and cannot be written",
                input.target.target_id, input.target.ownership
            ),
        ));
    }
    validate_projection_method(&input.target, input.method)?;
    Ok(())
}

pub(crate) struct PreparedProjectionStaging {
    pub(super) path: PathBuf,
    pub(super) expected_fingerprint: String,
    pub(super) expected_live_fingerprint: Option<String>,
}

pub(super) type PreparedOwnerGuard<'a> = dyn FnMut(&Path) -> Result<(), CommandFailure> + 'a;

impl PreparedProjectionStaging {
    pub(crate) fn new(
        path: PathBuf,
        expected_fingerprint: String,
        expected_live_fingerprint: Option<String>,
    ) -> Self {
        Self {
            path,
            expected_fingerprint,
            expected_live_fingerprint,
        }
    }
}

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
            ctx, paths, snapshot, input, None, None,
        )
        .map(|output| ProjectionExecutionOutput {
            projection: output.projection,
            prepared: None,
            backup: output.backup,
            commit: output.commit,
            meta: output.meta,
            noop: output.noop,
            activated: false,
        }),
        ProjectionExecutionContext::Convergence => {
            execute_projection_mode::<ConvergenceMode>(ctx, paths, snapshot, input, None, None)
        }
    }
}

pub(crate) fn prepare_convergence_projection(
    _ctx: &AppContext,
    input: &ProjectionExecutionInput,
    source: &Path,
    staging_path: &Path,
) -> Result<(), CommandFailure> {
    validate_projection_route(input)?;
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
    staging: impl Into<Option<PreparedProjectionStaging>>,
    mut validate_owner: impl FnMut(&Path) -> Result<(), CommandFailure>,
) -> Result<ProjectionExecutionOutput, CommandFailure> {
    let mut output = execute_projection_mode::<ConvergenceMode>(
        ctx,
        paths,
        snapshot,
        input,
        staging.into(),
        Some(&mut validate_owner),
    )?;
    let Some(prepared) = output.prepared.take() else {
        return Ok(output);
    };
    let mut activated = activate_prepared_projection(ctx, prepared)?;
    output.projection = Some(activated.finalize()?);
    output.activated = true;
    Ok(output)
}

#[inline(always)]
pub(crate) fn execute_standalone_projection(
    ctx: &AppContext,
    paths: &RegistryStatePaths,
    snapshot: &RegistrySnapshot,
    input: ProjectionExecutionInput,
) -> Result<StandaloneProjectionExecutionOutput, CommandFailure> {
    debug_assert_eq!(input.context, ProjectionExecutionContext::Standalone);
    execute_projection_mode::<StandaloneMode>(ctx, paths, snapshot, input, None, None)
}

pub(crate) fn finish_convergence_projection(_backup: Option<&Value>) -> Vec<Value> {
    Vec::new()
}
