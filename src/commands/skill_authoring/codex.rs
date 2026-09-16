use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    AuthoringRequest, GeneratedPatch, PatchKind, base_risk_notes, prompt_material_json,
    validation_plan,
};
use crate::commands::skill_authoring_patch::{ReviewedPatchFile, parse_patch_changes};
use crate::commands::skill_eval_harness::execute_reviewed_prompt;
use crate::commands::{CommandFailure, redact_sensitive_string};
use crate::state::AppContext;
use crate::types::ErrorCode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    patch: String,
    files: Vec<ChangedFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangedFile {
    path: String,
    change: String,
}

pub(super) fn generate_patch(
    ctx: &AppContext,
    request: &AuthoringRequest,
) -> Result<GeneratedPatch, CommandFailure> {
    let action_input = match &request.patch_kind {
        PatchKind::Draft { agent } => json!({"agent": agent}),
        PatchKind::Extract => json!({}),
        PatchKind::Instruction { path, body } => json!({"path": path, "body": body}),
        PatchKind::Rewrite { instruction } => json!({"instruction": instruction}),
        PatchKind::TuneDescription { description } => json!({"description": description}),
        PatchKind::GenerateEvals { task } => json!({"task": task}),
    };
    let prompt = format!(
        "Generate a reviewable Loom skill patch. Do not use tools, execute commands, or read other files. \
         Use only the reviewed, redacted input below. Return one JSON object with exactly \
         {{\"patch\":\"git unified diff\",\"files\":[{{\"path\":\"skills/{}/SKILL.md\",\"change\":\"add or modify\"}}]}}. \
         No markdown fences. Every path must be inside skills/{}/. Include diff --git, --- a/..., +++ b/..., \
         and correct @@ line counts. Only add or modify files; do not delete. \
         Preserve frontmatter name and all unrelated content. Include no secrets. \
         Action: {}\nGoal: {}\nRequested parameters: {}\nReviewed input:\n{}",
        request.skill,
        request.skill,
        request.action,
        request.goal,
        action_input,
        prompt_material_json(&request.prompt_sources),
    );
    let raw = execute_reviewed_prompt(None, &redact_sensitive_string(&prompt), false)?;
    let answer: Answer = serde_json::from_str(&raw).map_err(|_| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            "Codex answer must be JSON containing patch and files",
        )
    })?;
    if redact_sensitive_string(&answer.patch) != answer.patch {
        return Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "generated patch contains sensitive content",
        ));
    }
    let files = answer
        .files
        .iter()
        .map(|file| ReviewedPatchFile {
            path: file.path.clone(),
            change: file.change.clone(),
        })
        .collect::<Vec<_>>();
    // The same boundary used by apply checks paths, changes, and exact hunk applicability.
    parse_patch_changes(ctx, &request.skill, &answer.patch, &files)?;
    Ok(GeneratedPatch {
        patch_body: answer.patch,
        files: answer
            .files
            .iter()
            .map(|file| json!({"path": file.path, "change": file.change}))
            .collect(),
        validation_plan: validation_plan(&request.skill),
        risk_notes: base_risk_notes(request.action),
    })
}

pub(super) fn preview(request: &AuthoringRequest) -> Value {
    json!({
        "provider": "codex-cli", "dry_run": true, "artifact_written": false,
        "skill": request.skill, "action": request.action, "goal": request.goal,
        "prompt_material": prompt_material_json(&request.prompt_sources),
        "requires_review": true,
        "note": "Omit --dry-run to send this redacted material to the configured Codex CLI model and create a patch artifact; source files are applied separately.",
    })
}
