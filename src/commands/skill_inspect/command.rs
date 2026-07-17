use serde_json::json;

use crate::cli::SkillInspectArgs;
use crate::envelope::Meta;
use crate::next_action_trace::observe_next_actions;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::super::convergence_status::{ConvergenceRequest, collect_convergence_status};
use super::super::helpers::{map_arg, map_registry_state, validate_skill_name};
use super::super::skill_compile::compiled_artifact_summary;
use super::super::skill_deps::skill_dependency_report;
use super::super::skill_inventory::skill_brief_payload;
use super::super::skill_safety::trust_metadata_for_skill;
use super::super::telemetry::skill_telemetry_summary;
use super::super::{App, CommandFailure};
use super::evidence::{build_quality_evidence, build_safety_evidence};
use super::{
    Selector, build_next_actions, build_provenance_status, build_runtime_status,
    build_source_status, build_spec_status, push_unique, snapshot_references_skill,
};

impl App {
    pub fn cmd_skill_inspect(
        &self,
        args: &SkillInspectArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        if args.brief {
            let (mut data, mut meta) = skill_brief_payload(&self.ctx, &args.skill)?;
            let agent = args.agent.as_deref().map(str::to_ascii_lowercase);
            let convergence = collect_convergence_status(
                &self.ctx,
                ConvergenceRequest {
                    skill: Some(&args.skill),
                    agent: agent.as_deref(),
                    workspace: args.workspace.as_deref(),
                    profile: args.profile.as_deref(),
                },
            );
            data["convergence"] = json!(convergence.status);
            meta.sync_state = convergence.sync_state;
            meta.warnings.extend(convergence.warnings);
            return Ok((data, meta));
        }
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let agent = args.agent.as_ref().map(|agent| agent.to_ascii_lowercase());
        let selector = Selector {
            agent: agent.as_deref(),
            workspace: args.workspace.as_deref(),
            profile: args.profile.as_deref(),
        };
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
        let skill_path = self.ctx.skill_path(&args.skill);
        let source_exists = skill_path.is_dir();
        let referenced = snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot_references_skill(snapshot, &args.skill));

        if !source_exists && !referenced {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let source = build_source_status(&self.ctx, &args.skill, &skill_path, source_exists)?;
        let spec = build_spec_status(&self.ctx.root, &args.skill, &skill_path, source_exists);
        let provenance = build_provenance_status(&self.ctx, &args.skill, source_exists)?;
        let trust = trust_metadata_for_skill(&self.ctx, &args.skill)?;
        let dependencies = if source_exists {
            Some(skill_dependency_report(
                &self.ctx,
                &args.skill,
                selector.agent,
                selector.workspace,
            )?)
        } else {
            None
        };
        let runtime = build_runtime_status(
            &args.skill,
            &skill_path,
            source_exists,
            snapshot.as_ref(),
            selector,
        );
        let mut next_actions = build_next_actions(&args.skill, &spec, &runtime, source_exists);
        if spec.findings.iter().any(|finding| {
            finding["severity"].as_str() == Some("error")
                || finding["severity"].as_str() == Some("warning")
        }) {
            push_unique(
                &mut next_actions,
                format!("loom skill lint {} --portable", args.skill),
            );
        }
        push_unique(&mut next_actions, format!("loom skill eval {}", args.skill));
        push_unique(
            &mut next_actions,
            format!("loom skill policy {}", args.skill),
        );
        let telemetry = if args.include_telemetry {
            Some(skill_telemetry_summary(&self.ctx, &args.skill)?)
        } else {
            None
        };
        let compiled = compiled_artifact_summary(&self.ctx, &args.skill)?;
        let quality =
            build_quality_evidence(&self.ctx, &args.skill, source_exists, &source.drifted_paths);
        let safety = build_safety_evidence(
            &self.ctx,
            &args.skill,
            &trust,
            source_exists,
            snapshot.as_ref(),
            selector,
        );
        let convergence = collect_convergence_status(
            &self.ctx,
            ConvergenceRequest {
                skill: Some(&args.skill),
                agent: selector.agent,
                workspace: selector.workspace,
                profile: selector.profile,
            },
        );
        Ok((
            json!({
                "skill": args.skill,
                "source": source,
                "spec": spec,
                "provenance": provenance,
                "runtime": runtime,
                "dependencies": dependencies,
                "quality": quality,
                "safety": safety,
                "telemetry": telemetry,
                "compiled": compiled,
                "convergence": convergence.status,
                "next_actions": observe_next_actions("skill.inspect.response", next_actions),
            }),
            Meta {
                warnings: convergence.warnings,
                sync_state: convergence.sync_state,
                op_id: None,
            },
        ))
    }
}
