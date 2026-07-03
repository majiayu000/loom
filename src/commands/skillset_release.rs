use serde_json::{Value, json};

use crate::cli::{SkillsetReleaseArgs, SkillsetRollbackArgs};
use crate::envelope::Meta;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{commit_registry_state, map_git, map_lock, validate_non_empty};
use super::skillset_cmds::{
    SKILLSETS_REL, SkillsetRecord, SkillsetsFile, lint_skillset, load_skillsets,
    parse_skillsets_file, render_skillset, save_skillsets, skill_inventory_by_id,
    validate_skillset_id,
};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_skillset_release(
        &self,
        args: &SkillsetReleaseArgs,
        _request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        validate_refish_component("version", &args.version)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.ensure_registry_layout()?;
        ensure_clean_skillsets_definition(&self.ctx)?;
        let file = load_skillsets(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let inventory = skill_inventory_by_id(&self.ctx)?;
        let lint = lint_skillset(skillset, &inventory);
        if lint["valid"].as_bool() != Some(true) {
            let mut failure = CommandFailure::new(
                ErrorCode::PolicyBlocked,
                format!("skillset '{}' is not valid for release", args.name),
            );
            failure.details = json!({ "lint": lint });
            return Err(failure);
        }

        let tag = skillset_release_tag(&args.name, &args.version)?;
        gitops::create_annotated_tag(
            &self.ctx,
            &tag,
            &format!("release skillset {} {}", args.name, args.version),
        )
        .map_err(map_git)?;

        Ok((
            json!({
                "skillset": args.name,
                "version": args.version,
                "tag": tag,
                "released_ref": gitops::head(&self.ctx).map_err(map_git)?,
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_skillset_rollback(
        &self,
        args: &SkillsetRollbackArgs,
        _request_id: &str,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        validate_refish_component("ref", &args.to)?;
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.ensure_registry_layout()?;

        let reference = resolve_skillset_rollback_ref(&self.ctx, &args.name, &args.to)?;
        let source_file = load_skillsets_from_ref(&self.ctx, &reference)?;
        let replacement = source_file.find(&args.name).cloned().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found at ref '{}'", args.name, reference),
            )
        })?;
        let mut current = load_skillsets(&self.ctx)?;
        if current.find(&args.name).is_none() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!(
                    "skillset '{}' is not defined in the current registry",
                    args.name
                ),
            ));
        }
        let inventory = skill_inventory_by_id(&self.ctx)?;
        let before = current.clone();
        replace_skillset(&mut current, replacement.clone());
        if current == before {
            return Ok((
                json!({
                    "skillset": args.name,
                    "reference": reference,
                    "noop": true,
                    "skillset_record": render_skillset(&replacement, Some(&inventory)),
                }),
                Meta::default(),
            ));
        }

        save_skillsets(&self.ctx, &mut current)?;
        let commit = match commit_registry_state(
            &self.ctx,
            &format!("skillset({}): rollback to {}", args.name, reference),
        ) {
            Ok(commit) => commit,
            Err(err) => {
                let rollback_errors = restore_skillsets_file(&self.ctx, before);
                return Err(err.with_rollback_errors(rollback_errors));
            }
        };

        Ok((
            json!({
                "skillset": args.name,
                "reference": reference,
                "commit": commit,
                "noop": false,
                "skillset_record": render_skillset(&replacement, Some(&inventory)),
            }),
            Meta::default(),
        ))
    }
}

fn load_skillsets_from_ref(
    ctx: &AppContext,
    reference: &str,
) -> std::result::Result<SkillsetsFile, CommandFailure> {
    let spec = format!("{reference}:{SKILLSETS_REL}");
    let raw = gitops::run_git(ctx, &["show", &spec]).map_err(map_git)?;
    parse_skillsets_file(&raw, &spec)
}

fn replace_skillset(file: &mut SkillsetsFile, replacement: SkillsetRecord) {
    if let Some(existing) = file.find_mut(&replacement.id) {
        *existing = replacement;
    } else {
        file.skillsets.push(replacement);
    }
    file.normalize();
}

fn restore_skillsets_file(ctx: &AppContext, mut before: SkillsetsFile) -> Vec<Value> {
    let mut errors = Vec::new();
    if let Err(err) = save_skillsets(ctx, &mut before) {
        errors.push(json!({
            "step": "restore_skillsets_file",
            "message": err.message.clone(),
            "error": {
                "code": err.code.as_str(),
                "message": err.message,
                "details": err.details,
                "next_actions": err.next_actions,
            },
        }));
        return errors;
    }
    if let Err(err) = gitops::run_git(ctx, &["add", "--", SKILLSETS_REL]) {
        errors.push(json!({
            "step": "restore_skillsets_index",
            "message": err.to_string(),
        }));
    }
    errors
}

fn ensure_clean_skillsets_definition(ctx: &AppContext) -> std::result::Result<(), CommandFailure> {
    let output =
        gitops::run_git(ctx, &["status", "--porcelain", "--", SKILLSETS_REL]).map_err(map_git)?;
    if output.trim().is_empty() {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        "skillset release requires committed skillset definition state",
    );
    failure.details = json!({
        "path": SKILLSETS_REL,
        "status": output.lines().collect::<Vec<_>>(),
        "next_actions": [
            "commit or discard state/registry/skillsets.json changes before release"
        ],
    });
    Err(failure)
}

fn skillset_release_tag(name: &str, version: &str) -> std::result::Result<String, CommandFailure> {
    validate_refish_component("version", version)?;
    Ok(format!("release/skillset/{name}/{version}"))
}

fn resolve_skillset_rollback_ref(
    ctx: &AppContext,
    name: &str,
    raw: &str,
) -> std::result::Result<String, CommandFailure> {
    let release_tag = skillset_release_tag(name, raw)?;
    if gitops::resolve_ref(ctx, &release_tag).is_ok() {
        return Ok(release_tag);
    }
    validate_refish_component("ref", raw)?;
    gitops::resolve_ref(ctx, raw).map_err(map_git)?;
    Ok(raw.to_string())
}

fn validate_refish_component(label: &str, value: &str) -> std::result::Result<(), CommandFailure> {
    let trimmed = value.trim();
    validate_non_empty(label, trimmed)?;
    if trimmed != value {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("{label} must not include leading or trailing whitespace"),
        ));
    }
    if value.starts_with('-')
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || value.contains('\\')
        || value.contains(':')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.ends_with(".lock")
        || value
            .chars()
            .any(|ch| ch.is_ascii_control() || ch.is_whitespace())
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("{label} is not a safe Git ref or version token"),
        ));
    }
    Ok(())
}
