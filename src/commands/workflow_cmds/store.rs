use std::fs;
use std::path::PathBuf;

use crate::fs_util::write_atomic;
use crate::state::AppContext;
use crate::state_model::REGISTRY_SCHEMA_VERSION;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::model::{
    StoredWorkflowPlan, WORKFLOW_PLANS_REL, WORKFLOWS_REL, WorkflowPlansFile, WorkflowRecord,
    WorkflowsFile,
};

pub(super) fn find_workflow<'a>(
    file: &'a WorkflowsFile,
    workflow_id: &str,
) -> std::result::Result<&'a WorkflowRecord, CommandFailure> {
    file.find(workflow_id).ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("workflow '{}' not found", workflow_id),
        )
    })
}

pub(super) fn load_workflows(
    ctx: &AppContext,
) -> std::result::Result<WorkflowsFile, CommandFailure> {
    let path = workflows_path(ctx);
    if !path.exists() {
        return Ok(WorkflowsFile::empty());
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let mut file: WorkflowsFile = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", path.display(), err),
        )
    })?;
    if file.schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "{} schema version {} is not supported",
                path.display(),
                file.schema_version
            ),
        ));
    }
    file.normalize();
    Ok(file)
}

pub(super) fn save_workflows(
    ctx: &AppContext,
    file: &mut WorkflowsFile,
) -> std::result::Result<(), CommandFailure> {
    file.normalize();
    let path = workflows_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    let raw = serde_json::to_string_pretty(file).map_err(map_io)? + "\n";
    write_atomic(&path, &raw).map_err(map_io)
}

pub(super) fn load_workflow_plans(
    ctx: &AppContext,
) -> std::result::Result<WorkflowPlansFile, CommandFailure> {
    let path = workflow_plans_path(ctx);
    if !path.exists() {
        return Ok(WorkflowPlansFile::empty());
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let mut file: WorkflowPlansFile = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", path.display(), err),
        )
    })?;
    if file.schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "{} schema version {} is not supported",
                path.display(),
                file.schema_version
            ),
        ));
    }
    file.normalize();
    Ok(file)
}

pub(super) fn save_workflow_plan(
    ctx: &AppContext,
    plan: StoredWorkflowPlan,
) -> std::result::Result<(), CommandFailure> {
    let mut file = load_workflow_plans(ctx)?;
    file.plans.push(plan);
    file.normalize();
    let path = workflow_plans_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(map_io)?;
    }
    let raw = serde_json::to_string_pretty(&file).map_err(map_io)? + "\n";
    write_atomic(&path, &raw).map_err(map_io)
}

pub(super) fn workflows_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(WORKFLOWS_REL)
}

fn workflow_plans_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(WORKFLOW_PLANS_REL)
}
