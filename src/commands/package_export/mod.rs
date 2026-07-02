mod archive;
mod model;
mod source;

use std::fs;
use std::path::Path;

use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{
    PackageBuildArgs, PackageCommand, PackageFormatArg, PackagePlanArgs, PackageVerifyArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::types::ErrorCode;

use super::helpers::{map_io, shell_arg, validate_non_empty};
use super::{App, CommandFailure, SkillLintMode, lint_skill_source};
use archive::{build_archive, verify_archive};
use model::{
    PACKAGE_SCHEMA_VERSION, PackageBuildMetadata, PackageManifest, PackagePlan, SUPPORTED_FORMAT,
};
use source::{
    collect_source_files, package_checks, package_policy_blocked, plan_files,
    reject_output_inside_sources, resolve_package_source, source_digest,
};

impl App {
    pub fn cmd_package(
        &self,
        command: &PackageCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            PackageCommand::Plan(args) => self.cmd_package_plan(args),
            PackageCommand::Build(args) => self.cmd_package_build(args),
            PackageCommand::Verify(args) => self.cmd_package_verify(args),
        }
    }

    fn cmd_package_plan(
        &self,
        args: &PackagePlanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_supported_format(args.format)?;
        let source = resolve_package_source(&self.ctx, &args.source)?;
        let copy_files = collect_source_files(&self.ctx, &source)?;
        let checks = package_checks(&self.ctx, &source)?;
        let source_digest = source_digest(&source, &copy_files);
        let plan = PackagePlan {
            schema_version: PACKAGE_SCHEMA_VERSION,
            plan_id: format!("pkgplan_{}", Uuid::new_v4().simple()),
            created_at: Utc::now(),
            source,
            format: SUPPORTED_FORMAT.to_string(),
            loom_version: env!("CARGO_PKG_VERSION").to_string(),
            source_ref: gitops::head(&self.ctx).unwrap_or_else(|_| "working-tree".to_string()),
            source_digest,
            files: plan_files(&copy_files),
            checks,
            warnings: Vec::new(),
        };

        if let Some(output_plan) = &args.output_plan {
            reject_output_inside_sources(&self.ctx, output_plan, &plan.source)?;
            let mut body = serde_json::to_string_pretty(&plan).map_err(map_io)?;
            body.push('\n');
            crate::fs_util::write_atomic(output_plan, &body).map_err(map_io)?;
        }

        Ok((
            json!({
                "plan": plan,
                "output_plan": args.output_plan.as_ref().map(|path| path.display().to_string()),
                "supported_formats": [SUPPORTED_FORMAT],
                "deferred_formats": ["codex-plugin", "claude-plugin", "npm", "github-release"],
            }),
            Meta::default(),
        ))
    }

    fn cmd_package_build(
        &self,
        args: &PackageBuildArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_non_empty("idempotency-key", &args.idempotency_key)?;
        let plan = load_plan_artifact(&args.plan)?;
        ensure_plan_supported(&plan)?;
        if args.output.exists() {
            return self.replay_existing_package_build(args, &plan);
        }
        let current_source = resolve_package_source(
            &self.ctx,
            &format!("{}:{}", plan.source.kind, plan.source.id),
        )?;
        let copy_files = collect_source_files(&self.ctx, &current_source)?;
        let current_digest = source_digest(&current_source, &copy_files);
        if current_digest != plan.source_digest {
            return Err(package_policy_blocked(
                "package source digest changed; create a new package plan",
                json!({"expected": plan.source_digest, "actual": current_digest}),
            ));
        }
        let current_checks = package_checks(&self.ctx, &current_source)?;
        reject_output_inside_sources(&self.ctx, &args.output, &current_source)?;
        let artifact_root = format!("loom-package-{}", plan.plan_id);
        let manifest = PackageManifest {
            schema_version: PACKAGE_SCHEMA_VERSION,
            plan_id: plan.plan_id.clone(),
            created_at: plan.created_at,
            source: current_source,
            format: plan.format.clone(),
            loom_version: plan.loom_version.clone(),
            source_ref: plan.source_ref.clone(),
            source_digest: plan.source_digest.clone(),
            files: plan_files(&copy_files),
            checks: current_checks,
            build: PackageBuildMetadata {
                artifact_root: artifact_root.clone(),
            },
        };
        build_archive(&args.output, &artifact_root, &manifest, &copy_files)?;
        Ok((
            json!({
                "artifact": args.output.display().to_string(),
                "format": plan.format,
                "manifest": manifest,
                "verify_command": format!("loom package verify {}", shell_arg(&args.output)),
                "install_guidance": "install or publish this artifact outside Loom, then verify agent visibility separately",
                "active_state_claim": false,
            }),
            Meta::default(),
        ))
    }

    fn cmd_package_verify(
        &self,
        args: &PackageVerifyArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        Ok((verify_archive(&self.ctx, args)?, Meta::default()))
    }

    fn replay_existing_package_build(
        &self,
        args: &PackageBuildArgs,
        plan: &PackagePlan,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let verify_args = PackageVerifyArgs {
            artifact: args.output.clone(),
            format: Some(PackageFormatArg::AgentSkillsArchive),
        };
        let verified = verify_archive(&self.ctx, &verify_args)?;
        let manifest = &verified["manifest"];
        if manifest["plan_id"] != json!(plan.plan_id)
            || manifest["format"] != json!(plan.format)
            || manifest["source_digest"] != json!(plan.source_digest)
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "package artifact already exists for a different plan: {}",
                    args.output.display()
                ),
            ));
        }
        Ok((
            json!({
                "artifact": args.output.display().to_string(),
                "format": plan.format,
                "manifest": manifest,
                "verify_command": format!("loom package verify {}", shell_arg(&args.output)),
                "install_guidance": "install or publish this artifact outside Loom, then verify agent visibility separately",
                "active_state_claim": false,
                "idempotent_replay": true,
            }),
            Meta::default(),
        ))
    }
}

pub(super) fn ensure_supported_format(
    format: PackageFormatArg,
) -> std::result::Result<(), CommandFailure> {
    if format == PackageFormatArg::AgentSkillsArchive {
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::ArgInvalid,
        format!(
            "package format '{}' is not supported in this slice; use agent-skills-archive",
            package_format_as_str(format)
        ),
    ))
}

fn ensure_plan_supported(plan: &PackagePlan) -> std::result::Result<(), CommandFailure> {
    if plan.schema_version != PACKAGE_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported package plan schema_version {}",
                plan.schema_version
            ),
        ));
    }
    if plan.format != SUPPORTED_FORMAT {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsupported package plan format '{}'", plan.format),
        ));
    }
    Ok(())
}

fn load_plan_artifact(raw: &str) -> std::result::Result<PackagePlan, CommandFailure> {
    let path = Path::new(raw);
    if !path.is_file() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "package build currently requires an explicit plan artifact path",
        ));
    }
    let plan: PackagePlan = serde_json::from_slice(&fs::read(path).map_err(map_io)?)
        .map_err(|err| CommandFailure::new(ErrorCode::StateCorrupt, err.to_string()))?;
    ensure_plan_supported(&plan)?;
    Ok(plan)
}

pub(super) fn package_format_as_str(format: PackageFormatArg) -> &'static str {
    match format {
        PackageFormatArg::AgentSkillsArchive => "agent-skills-archive",
        PackageFormatArg::CodexPlugin => "codex-plugin",
        PackageFormatArg::ClaudePlugin => "claude-plugin",
        PackageFormatArg::Npm => "npm",
        PackageFormatArg::GithubRelease => "github-release",
    }
}
