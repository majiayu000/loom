use crate::commands::{App, CommandFailure};
use crate::envelope::Meta;
use crate::state::AppContext;

use super::vocab::ProjectionMethod;

pub(crate) struct ProjectSkillInput {
    pub skill: String,
    pub binding: String,
    pub target: Option<String>,
    pub method: ProjectionMethod,
}

pub(crate) struct CommitProjectionInput {
    pub skill: String,
    pub binding: Option<String>,
    pub instance: Option<String>,
    pub message: Option<String>,
}

pub(crate) fn project_skill(
    ctx: &AppContext,
    input: ProjectSkillInput,
    request_id: &str,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let app = App { ctx: ctx.clone() };
    app.cmd_project(
        &crate::cli::ProjectArgs {
            skill: input.skill,
            binding: input.binding,
            target: input.target,
            method: input.method,
            dry_run: false,
        },
        request_id,
    )
}

pub(crate) fn commit_projection(
    ctx: &AppContext,
    input: CommitProjectionInput,
    request_id: &str,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let app = App { ctx: ctx.clone() };
    app.cmd_commit(
        &crate::cli::SkillCommitArgs {
            skill: input.skill,
            message: input.message,
            from_projection: true,
            from_source: false,
            binding: input.binding,
            instance: input.instance,
            preflight: false,
        },
        request_id,
    )
}
