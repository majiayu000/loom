use super::{
    AuthoringRequest, PatchKind, add_file_patch, redact_prompt_material, run_authoring_command,
};
use crate::commands::skill_authoring_patch::{ReviewedPatchFile, parse_patch_changes};
use crate::commands::{
    CommandFailure,
    helpers::{ensure_skill_exists, map_io},
};
use crate::{
    cli::{InstructionMigrationTarget, SkillAuthoringProviderArg},
    envelope::Meta,
    state::AppContext,
    types::ErrorCode,
};
use serde_json::Value;
use std::{fs, path::Path};

pub(crate) fn create_instruction_patch(
    ctx: &AppContext,
    source: &Path,
    skill: &str,
    reference: &str,
    target: InstructionMigrationTarget,
    dry_run: bool,
) -> Result<(Value, Meta), CommandFailure> {
    let body = fs::read_to_string(source).map_err(map_io)?;
    if redact_prompt_material(&body) != body {
        return Err(CommandFailure::new(
            ErrorCode::PolicyBlocked,
            "instruction contains sensitive content; remove it before migration",
        ));
    }
    let (path, content) = match target {
        InstructionMigrationTarget::Skill => (
            format!("skills/{skill}/SKILL.md"),
            format!(
                "---\nname: {skill}\ndescription: Use when applying the {skill} project workflow.\n---\n# {skill}\n\n{body}\n"
            ),
        ),
        InstructionMigrationTarget::Reference => {
            ensure_skill_exists(ctx, skill)?;
            (format!("skills/{skill}/references/{reference}.md"), body)
        }
        InstructionMigrationTarget::KeepInstruction => {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "keep-instruction needs no patch",
            ));
        }
    };
    parse_patch_changes(
        ctx,
        skill,
        &add_file_patch(&path, &content),
        &[ReviewedPatchFile {
            path: path.clone(),
            change: "add".to_string(),
        }],
    )?;
    run_authoring_command(
        ctx,
        SkillAuthoringProviderArg::Mock,
        dry_run,
        AuthoringRequest {
            action: "migrate-instruction",
            skill: skill.to_string(),
            goal: "Extract the reviewed instruction without modifying its source".to_string(),
            prompt_sources: Vec::new(),
            patch_kind: PatchKind::Instruction {
                path,
                body: content,
            },
        },
    )
}
