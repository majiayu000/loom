use crate::commands::{App, CommandFailure};
use crate::envelope::Meta;
use crate::state::AppContext;

pub(crate) struct CommitSourceInput {
    pub skill: String,
    pub message: Option<String>,
}

pub(crate) struct ReleaseAnchorInput {
    pub skill: String,
}

pub(crate) struct ReleaseVersionInput {
    pub skill: String,
    pub version: String,
}

pub(crate) struct RollbackInput {
    pub skill: String,
    pub to: Option<String>,
    pub steps: Option<u32>,
}

pub(crate) fn commit_source(
    ctx: &AppContext,
    input: CommitSourceInput,
    request_id: &str,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let app = App { ctx: ctx.clone() };
    app.cmd_commit(
        &crate::cli::SkillCommitArgs {
            skill: input.skill,
            message: input.message,
            from_projection: false,
            from_source: true,
            binding: None,
            instance: None,
            preflight: false,
        },
        request_id,
    )
}

pub(crate) fn release_anchor(
    ctx: &AppContext,
    input: ReleaseAnchorInput,
    request_id: &str,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let app = App { ctx: ctx.clone() };
    app.cmd_release(
        &crate::cli::ReleaseArgs {
            skill: input.skill,
            version: None,
            anchor: true,
            preflight: false,
            baseline: None,
        },
        request_id,
    )
}

pub(crate) fn release_version(
    ctx: &AppContext,
    input: ReleaseVersionInput,
    request_id: &str,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let app = App { ctx: ctx.clone() };
    app.cmd_release(
        &crate::cli::ReleaseArgs {
            skill: input.skill,
            version: Some(input.version),
            anchor: false,
            preflight: false,
            baseline: None,
        },
        request_id,
    )
}

pub(crate) fn rollback(
    ctx: &AppContext,
    input: RollbackInput,
    request_id: &str,
) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
    let app = App { ctx: ctx.clone() };
    app.cmd_rollback(
        &crate::cli::RollbackArgs {
            skill: input.skill,
            to: input.to,
            steps: input.steps,
            dry_run: false,
        },
        request_id,
    )
}
