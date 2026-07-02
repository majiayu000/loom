mod model;
mod plan;
mod util;
mod verify;

use std::fs;

use serde_json::{Value, json};

use crate::cli::{
    SkillCompileArgs, SkillCompileCommand, SkillCompileListArgs, SkillCompileVerifyArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::types::ErrorCode;

use super::helpers::{map_arg, validate_skill_name};
use super::skill_verify::head_tree_oid_for_path;
use super::{App, CommandFailure};
use model::{ArtifactStatus, COMPILE_SCHEMA_VERSION, COMPILER_VERSION, CompiledArtifactManifest};
use plan::{compile_gates, planned_artifact, source_digest_info};
use util::{
    artifact_ids, compiled_skill_root, ensure_skill_source_exists, validate_agent_selector,
    validate_artifact_id,
};
use verify::verify_artifact;

impl App {
    pub fn cmd_skill_compile(
        &self,
        args: &SkillCompileArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match &args.command {
            Some(SkillCompileCommand::List(args)) => self.cmd_skill_compile_list(args),
            Some(SkillCompileCommand::Verify(args)) => self.cmd_skill_compile_verify(args),
            None => self.cmd_skill_compile_dry_run(args),
        }
    }

    fn cmd_skill_compile_dry_run(
        &self,
        args: &SkillCompileArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if !args.dry_run {
            return Err(CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "compiled artifact writes are deferred in this slice; pass --dry-run",
            ));
        }
        let skill = compile_skill_selector(args)?;
        validate_agent_selector("agent", &args.agent)?;
        validate_agent_selector("profile", &args.profile)?;
        ensure_skill_source_exists(&self.ctx, skill)?;

        let source = source_digest_info(&self.ctx, skill, &args.agent, &args.profile)?;
        let planned = planned_artifact(&self.ctx, skill, &args.agent, &args.profile, &source)?;
        let gates = compile_gates(&self.ctx, skill, &args.agent);
        let status = if gates.has_blocking_failure() {
            ArtifactStatus::Blocked
        } else {
            ArtifactStatus::Planned
        };
        let manifest = CompiledArtifactManifest {
            schema_version: COMPILE_SCHEMA_VERSION,
            artifact_id: planned.artifact_id.clone(),
            skill: skill.to_string(),
            agent: args.agent.clone(),
            profile: args.profile.clone(),
            source_ref: gitops::head(&self.ctx).unwrap_or_else(|_| "working-tree".to_string()),
            source_tree_oid: head_tree_oid_for_path(&self.ctx, &format!("skills/{skill}"))
                .unwrap_or(None),
            source_digest: source.digest.clone(),
            compiler_version: COMPILER_VERSION.to_string(),
            status,
            gates: gates.manifest.clone(),
            content_hashes: planned.content_hashes.clone(),
            token_estimate: planned.token_estimate.clone(),
            created_at: None,
        };
        let artifact_id = manifest.artifact_id.clone();
        Ok((
            json!({
                "skill": skill,
                "agent": args.agent,
                "profile": args.profile,
                "dry_run": true,
                "writes_artifacts": false,
                "no_op": planned.no_op,
                "no_op_reason": planned.no_op_reason,
                "artifact": {
                    "artifact_id": planned.artifact_id,
                    "layout_root": planned.artifact_root.display().to_string(),
                    "paths": planned.paths,
                },
                "source": {
                    "digest": source.digest,
                    "digest_inputs": source.inputs,
                },
                "gates": gates.details,
                "manifest": manifest,
                "planned_content": planned.content,
                "next_actions": [
                    format!("loom skill compile verify {skill} --artifact {artifact_id}")
                ],
            }),
            Meta::default(),
        ))
    }

    fn cmd_skill_compile_list(
        &self,
        args: &SkillCompileListArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_source_exists(&self.ctx, &args.skill)?;
        let root = compiled_skill_root(&self.ctx, &args.skill);
        let mut artifacts = Vec::new();
        for artifact_id in artifact_ids(&root)? {
            let dir = root.join(&artifact_id);
            let manifest_path = dir.join("manifest.json");
            let manifest = fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<CompiledArtifactManifest>(&raw).ok());
            artifacts.push(json!({
                "artifact_id": artifact_id,
                "path": dir.display().to_string(),
                "manifest_present": manifest_path.is_file(),
                "manifest_status": if manifest.is_some() {
                    "parseable"
                } else if manifest_path.is_file() {
                    "malformed"
                } else {
                    "missing"
                },
                "status": manifest.as_ref().map(|manifest| manifest.status.as_str()),
                "agent": manifest.as_ref().map(|manifest| manifest.agent.as_str()),
                "profile": manifest.as_ref().map(|manifest| manifest.profile.as_str()),
                "source_digest": manifest.as_ref().map(|manifest| manifest.source_digest.as_str()),
            }));
        }
        Ok((
            json!({
                "skill": args.skill,
                "artifact_root": root.display().to_string(),
                "artifacts": artifacts,
                "count": artifacts.len(),
            }),
            Meta::default(),
        ))
    }

    fn cmd_skill_compile_verify(
        &self,
        args: &SkillCompileVerifyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_source_exists(&self.ctx, &args.skill)?;
        let root = compiled_skill_root(&self.ctx, &args.skill);
        let artifact_ids = match &args.artifact {
            Some(artifact_id) => {
                validate_artifact_id(artifact_id)?;
                vec![artifact_id.clone()]
            }
            None => artifact_ids(&root)?,
        };
        let mut reports = Vec::new();
        let mut all_valid = true;
        for artifact_id in artifact_ids {
            let report = verify_artifact(&self.ctx, &args.skill, &root, &artifact_id)?;
            all_valid &= report.valid;
            reports.push(report);
        }
        Ok((
            json!({
                "skill": args.skill,
                "artifact_root": root.display().to_string(),
                "artifact": args.artifact,
                "valid": all_valid,
                "artifacts": reports,
                "count": reports.len(),
            }),
            Meta::default(),
        ))
    }
}

fn compile_skill_selector(args: &SkillCompileArgs) -> std::result::Result<&str, CommandFailure> {
    match (args.skill.as_deref(), args.skill_selector.as_deref()) {
        (Some(_), Some(_)) => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provide either positional <skill> or --skill, not both",
        )),
        (Some(skill), None) | (None, Some(skill)) => {
            validate_skill_name(skill).map_err(map_arg)?;
            Ok(skill)
        }
        (None, None) => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "skill compile --dry-run requires <skill> or --skill <skill>",
        )),
    }
}
