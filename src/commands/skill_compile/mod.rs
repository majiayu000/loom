mod model;
mod plan;
mod util;
mod verify;

use std::fs;

use chrono::Utc;
use serde_json::{Value, json};

use crate::cli::{
    SkillCompileArgs, SkillCompileCommand, SkillCompileListArgs, SkillCompileVerifyArgs,
};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_git, map_io, map_lock, validate_skill_name};
use super::skill_verify::head_tree_oid_for_path;
use super::{App, CommandFailure};
use model::{
    ArtifactStatus, COMPILE_SCHEMA_VERSION, COMPILER_VERSION, CompiledArtifactManifest, GatePlan,
    PlannedArtifact, REQUIRED_ARTIFACT_FILES, SourceDigestInfo,
};
use plan::{compile_gates, planned_artifact, source_digest_info};
use util::{
    artifact_ids, compiled_skill_root, ensure_skill_source_exists, stable_json,
    validate_agent_selector, validate_artifact_id,
};
use verify::verify_artifact;

struct CompilePlan {
    source: SourceDigestInfo,
    planned: PlannedArtifact,
    gates: GatePlan,
    manifest: CompiledArtifactManifest,
}

enum ManifestRead {
    Missing,
    Parseable(Box<CompiledArtifactManifest>),
    Malformed(String),
}

impl App {
    pub fn cmd_skill_compile(
        &self,
        args: &SkillCompileArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match &args.command {
            Some(SkillCompileCommand::List(args)) => self.cmd_skill_compile_list(args),
            Some(SkillCompileCommand::Verify(args)) => self.cmd_skill_compile_verify(args),
            None if args.dry_run => self.cmd_skill_compile_dry_run(args),
            None => self.cmd_skill_compile_write(args),
        }
    }

    fn cmd_skill_compile_dry_run(
        &self,
        args: &SkillCompileArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if !args.dry_run {
            return Err(CommandFailure::new(
                ErrorCode::PolicyBlocked,
                "internal compile routing expected --dry-run",
            ));
        }
        let skill = compile_skill_selector(args)?;
        validate_agent_selector("agent", &args.agent)?;
        validate_agent_selector("profile", &args.profile)?;
        ensure_skill_source_exists(&self.ctx, skill)?;

        let plan = self.build_compile_plan(skill, args, None, true)?;
        let artifact_id = plan.manifest.artifact_id.clone();
        Ok((
            json!({
                "skill": skill,
                "agent": args.agent,
                "profile": args.profile,
                "dry_run": true,
                "writes_artifacts": false,
                "no_op": plan.planned.no_op,
                "no_op_reason": plan.planned.no_op_reason,
                "artifact": {
                    "artifact_id": plan.planned.artifact_id,
                    "layout_root": plan.planned.artifact_root.display().to_string(),
                    "paths": plan.planned.paths,
                },
                "source": {
                    "digest": plan.source.digest,
                    "digest_inputs": plan.source.inputs,
                },
                "gates": plan.gates.details,
                "manifest": plan.manifest,
                "planned_content": plan.planned.content,
                "next_actions": [
                    format!("loom skill compile verify {skill} --artifact {artifact_id}")
                ],
            }),
            Meta::default(),
        ))
    }

    fn cmd_skill_compile_write(
        &self,
        args: &SkillCompileArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let skill = compile_skill_selector(args)?;
        validate_agent_selector("agent", &args.agent)?;
        validate_agent_selector("profile", &args.profile)?;
        ensure_skill_source_exists(&self.ctx, skill)?;

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;

        let existing_created_at = existing_created_at(&self.ctx, skill, args)?;
        let plan = self.build_compile_plan(skill, args, existing_created_at, false)?;
        write_compiled_artifact(&plan.planned, &plan.manifest)?;

        let root = compiled_skill_root(&self.ctx, skill);
        let verification = verify_artifact(&self.ctx, skill, &root, &plan.manifest.artifact_id)?;
        let commit_path = format!(
            "state/compiled/skills/{skill}/{}",
            plan.manifest.artifact_id
        );
        let commit = match gitops::commit_paths_if_changed(
            &self.ctx,
            &[commit_path.as_str(), ".gitignore"],
            &format!("skill({skill}): write compiled artifact"),
        ) {
            Ok(Some(commit)) => commit,
            Ok(None) => gitops::head(&self.ctx).map_err(map_git)?,
            Err(err) => return Err(map_git(err)),
        };
        let artifact_id = plan.manifest.artifact_id.clone();
        Ok((
            json!({
                "skill": skill,
                "agent": args.agent,
                "profile": args.profile,
                "dry_run": false,
                "writes_artifacts": true,
                "no_op": plan.planned.no_op,
                "no_op_reason": plan.planned.no_op_reason,
                "artifact": {
                    "artifact_id": plan.planned.artifact_id,
                    "layout_root": plan.planned.artifact_root.display().to_string(),
                    "paths": plan.planned.paths,
                },
                "source": {
                    "digest": plan.source.digest,
                    "digest_inputs": plan.source.inputs,
                },
                "gates": plan.gates.details,
                "manifest": plan.manifest,
                "verification": verification,
                "commit": commit,
                "next_actions": [
                    format!("loom skill compile verify {skill} --artifact {artifact_id}"),
                    format!("loom skill inspect {skill}")
                ],
            }),
            Meta::default(),
        ))
    }

    fn build_compile_plan(
        &self,
        skill: &str,
        args: &SkillCompileArgs,
        created_at: Option<String>,
        dry_run: bool,
    ) -> std::result::Result<CompilePlan, CommandFailure> {
        let source = source_digest_info(&self.ctx, skill, &args.agent, &args.profile)?;
        let planned = planned_artifact(&self.ctx, skill, &args.agent, &args.profile, &source)?;
        let gates = compile_gates(&self.ctx, skill, &args.agent);
        let status = artifact_status(&gates, dry_run);
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
            created_at: created_at.or_else(|| {
                if dry_run {
                    None
                } else {
                    Some(Utc::now().to_rfc3339())
                }
            }),
        };
        Ok(CompilePlan {
            source,
            planned,
            gates,
            manifest,
        })
    }

    fn cmd_skill_compile_list(
        &self,
        args: &SkillCompileListArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        ensure_skill_source_exists(&self.ctx, &args.skill)?;
        Ok((
            compiled_artifact_summary(&self.ctx, &args.skill)?,
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

pub(super) fn compiled_artifact_summary(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<Value, CommandFailure> {
    let root = compiled_skill_root(ctx, skill);
    let mut artifacts = Vec::new();
    for artifact_id in artifact_ids(&root)? {
        let dir = root.join(&artifact_id);
        let manifest_path = dir.join("manifest.json");
        let manifest = read_optional_manifest(&manifest_path)?;
        let (manifest_status, parsed, manifest_error) = match &manifest {
            ManifestRead::Missing => ("missing", None, None),
            ManifestRead::Parseable(manifest) => ("parseable", Some(manifest.as_ref()), None),
            ManifestRead::Malformed(err) => ("malformed", None, Some(err.as_str())),
        };
        artifacts.push(json!({
            "artifact_id": artifact_id,
            "path": dir.display().to_string(),
            "manifest_present": manifest_path.is_file(),
            "manifest_status": manifest_status,
            "manifest_error": manifest_error,
            "status": parsed.map(|manifest| manifest.status.as_str()),
            "agent": parsed.map(|manifest| manifest.agent.as_str()),
            "profile": parsed.map(|manifest| manifest.profile.as_str()),
            "source_digest": parsed.map(|manifest| manifest.source_digest.as_str()),
            "created_at": parsed.and_then(|manifest| manifest.created_at.as_deref()),
        }));
    }
    Ok(json!({
        "skill": skill,
        "artifact_root": root.display().to_string(),
        "artifacts": artifacts,
        "count": artifacts.len(),
    }))
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
            "skill compile requires <skill> or --skill <skill>",
        )),
    }
}

fn artifact_status(gates: &GatePlan, dry_run: bool) -> ArtifactStatus {
    if gates.has_blocking_failure() {
        ArtifactStatus::Blocked
    } else if dry_run {
        ArtifactStatus::Planned
    } else {
        ArtifactStatus::Experimental
    }
}

fn existing_created_at(
    ctx: &AppContext,
    skill: &str,
    args: &SkillCompileArgs,
) -> std::result::Result<Option<String>, CommandFailure> {
    let source = source_digest_info(ctx, skill, &args.agent, &args.profile)?;
    let planned = planned_artifact(ctx, skill, &args.agent, &args.profile, &source)?;
    let manifest_path = planned.artifact_root.join("manifest.json");
    let Some(manifest) = read_manifest_for_existing(&manifest_path)? else {
        return Ok(None);
    };
    Ok(manifest.created_at)
}

fn read_manifest_for_existing(
    path: &std::path::Path,
) -> std::result::Result<Option<CompiledArtifactManifest>, CommandFailure> {
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).map_err(map_io)?;
    serde_json::from_str(&raw).map(Some).map_err(|err| {
        let mut failure = CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "malformed existing compiled artifact manifest: {}",
                path.display()
            ),
        );
        failure.details = json!({
            "path": path.display().to_string(),
            "error": err.to_string(),
        });
        failure
    })
}

fn read_optional_manifest(
    path: &std::path::Path,
) -> std::result::Result<ManifestRead, CommandFailure> {
    if !path.is_file() {
        return Ok(ManifestRead::Missing);
    }
    let raw = fs::read_to_string(path).map_err(map_io)?;
    Ok(match serde_json::from_str(&raw) {
        Ok(manifest) => ManifestRead::Parseable(Box::new(manifest)),
        Err(err) => ManifestRead::Malformed(err.to_string()),
    })
}

fn write_compiled_artifact(
    planned: &PlannedArtifact,
    manifest: &CompiledArtifactManifest,
) -> std::result::Result<(), CommandFailure> {
    for (file, _) in REQUIRED_ARTIFACT_FILES {
        write_planned_file(planned, file)?;
    }
    write_planned_file(planned, "source-digest.txt")?;
    let manifest_raw = stable_json(manifest)?;
    write_atomic(&planned.artifact_root.join("manifest.json"), &manifest_raw).map_err(map_io)?;
    Ok(())
}

fn write_planned_file(
    planned: &PlannedArtifact,
    file: &str,
) -> std::result::Result<(), CommandFailure> {
    let value = planned.content.get(file).ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!("planned compiled artifact content missing {file}"),
        )
    })?;
    let body = if file.ends_with(".json") {
        stable_json(value)?
    } else {
        value
            .as_str()
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::SchemaMismatch,
                    format!("planned compiled artifact content for {file} must be text"),
                )
            })?
            .to_string()
    };
    write_atomic(&planned.artifact_root.join(file), &body).map_err(map_io)
}
