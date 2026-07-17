use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::PlanConvergeArgs;
use crate::core::convergence::{
    ConvergenceAxis, ConvergenceInputConflict, ConvergenceInputDirection, ConvergenceInputEvidence,
    ConvergenceSelectors, ProjectionEffectPlan, ProjectionInputEvidence, ProjectionInputState,
    RegistryGuard, RemotePolicy, SkillConvergencePlan, SourceGuard, VisibilityRequirement,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::{RegistryProjectionInstance, RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::agent_cmds::planning_helpers::{normalize_path, workspace_matches};
use super::super::convergence_input::{
    projection_input_evidence, source_changed_since_revision, source_dirty_paths,
};
use super::super::helpers::{
    map_arg, map_git, map_io, map_registry_state, projection_instance_id, validate_skill_name,
};
use super::super::projections::observe_projection;
use super::super::provenance::skill_tree_digest;
use super::super::skill_improve::prepare_convergence_skill_input;
use super::super::{App, CommandFailure};
use super::{PLAN_PROTOCOL_VERSION, canonical_root, policy_risks, required_approvals};

const CONVERGENCE_PLAN_SCHEMA_VERSION: &str = "1.2";

impl App {
    pub(super) fn cmd_plan_converge(
        &self,
        args: &PlanConvergeArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        if !self.ctx.skill_path(&args.skill).is_dir() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let workspace = args.workspace.as_ref().map(|path| normalize_path(path));
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
        let source_digest = skill_tree_digest(&self.ctx.skill_path(&args.skill)).map_err(map_io)?;
        let registry_head = gitops::head(&self.ctx).map_err(map_git)?;
        let mut projections = resolve_projection_effects(
            &self.ctx,
            snapshot.as_ref(),
            args,
            workspace.as_deref(),
            &source_digest,
        )?;
        validate_projection_input(args, &projections)?;
        let source_dirty_paths = source_dirty_paths(&self.ctx, &args.skill)?;
        let projection_evidence =
            resolve_projection_input_evidence(&self.ctx, snapshot.as_ref(), &projections)?;
        let direction = if args.from_projection {
            ConvergenceInputDirection::Projection
        } else {
            ConvergenceInputDirection::Source
        };
        let (selected_input_tree_digest, candidate_path) =
            selected_input(args, &projection_evidence, &source_digest)?;
        for effect in &mut projections {
            effect.source_tree_digest = selected_input_tree_digest.clone();
        }
        let candidate_method = args.instance.as_deref().and_then(|instance| {
            projection_evidence
                .iter()
                .find(|item| item.instance_id == instance)
                .map(|item| item.method.as_str())
        });
        let prepared_input = prepare_convergence_skill_input(
            &self.ctx,
            &args.skill,
            candidate_path.as_deref(),
            candidate_path.as_ref().and(candidate_method),
            &selected_input_tree_digest,
        )?;
        let policy = prepared_input.policy();
        let approvals = required_approvals(policy);
        let preflight_candidate = prepared_input.candidate_path(&args.skill);
        let preflight = self.convergence_preflight_evidence(
            &args.skill,
            args.agent.map(|agent| agent.as_str()),
            workspace.as_deref(),
            direction.clone(),
            &selected_input_tree_digest,
            preflight_candidate.as_deref(),
        )?;
        let selected_source_drift = if direction == ConvergenceInputDirection::Projection {
            !source_dirty_paths.is_empty()
                || projection_evidence
                    .iter()
                    .find(|item| item.instance_id == args.instance.as_deref().unwrap_or_default())
                    .and_then(|item| item.baseline_revision.as_deref())
                    .map(|revision| source_changed_since_revision(&self.ctx, &args.skill, revision))
                    .transpose()?
                    .unwrap_or(false)
        } else {
            false
        };
        let input_conflicts = resolve_input_conflicts(
            &source_dirty_paths,
            &projection_evidence,
            &preflight,
            &direction,
            args.instance.as_deref(),
            selected_source_drift,
        );
        let visibility = projections
            .iter()
            .map(|effect| VisibilityRequirement {
                agent: effect.agent.clone(),
                binding_id: effect.binding_id.clone(),
                target_id: effect.target_id.clone(),
                check: "post_apply_adapter_read".to_string(),
                required: args.require_runtime,
            })
            .collect::<Vec<_>>();
        let mut required_axes = BTreeSet::from([ConvergenceAxis::Projections]);
        if args.require_runtime {
            required_axes.insert(ConvergenceAxis::Visibility);
        }
        if args.push_remote {
            required_axes.insert(ConvergenceAxis::RegistryTransport);
        }

        let plan_id = format!("plan_{}", Uuid::new_v4().simple());
        let mut plan = SkillConvergencePlan {
            plan_id,
            plan_digest: String::new(),
            skill: args.skill.clone(),
            selectors: ConvergenceSelectors {
                agent: args.agent.map(|agent| agent.as_str().to_string()),
                workspace: workspace.map(|path| path.display().to_string()),
                profile: args.profile.clone(),
                input_instance: args.instance.clone(),
            },
            source: SourceGuard {
                direction,
                registry_head,
                tree_digest: source_digest.clone(),
                input_instance: args.instance.clone(),
            },
            input: ConvergenceInputEvidence {
                source_dirty_paths,
                projections: projection_evidence,
                selected_projection_instance: args.instance.clone(),
                selected_input_tree_digest,
            },
            preflight,
            input_conflicts,
            registry: registry_guard(snapshot.as_ref())?,
            projections,
            visibility,
            accept_restart_required: args.accept_restart_required,
            remote: if args.push_remote {
                RemotePolicy::Push
            } else {
                RemotePolicy::NotRequested
            },
            required_axes,
            required_approvals: approvals,
        };
        plan.seal().map_err(map_io)?;

        let mut conflicts = plan
            .input_conflicts
            .iter()
            .map(|conflict| serde_json::to_value(conflict).map_err(map_io))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if args.require_runtime && plan.projections.is_empty() {
            conflicts.push(json!({
                "code": "RUNTIME_PROJECTION_REQUIRED",
                "message": "--require-runtime resolved no active projection",
            }));
        }
        let mut risks = policy_risks(policy);
        risks.push(json!({
            "code": "CONVERGENCE_EXECUTOR_UNAVAILABLE",
            "risk_level": "error",
            "blocks_apply": true,
            "details": "this tranche persists reviewed convergence plans but does not execute them",
        }));

        let mut output = serde_json::to_value(&plan).map_err(map_io)?;
        let object = output.as_object_mut().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "typed convergence plan did not serialize as an object",
            )
        })?;
        object.insert("protocol_version".to_string(), json!(PLAN_PROTOCOL_VERSION));
        object.insert(
            "schema_version".to_string(),
            json!(CONVERGENCE_PLAN_SCHEMA_VERSION),
        );
        object.insert("operation".to_string(), json!("converge"));
        object.insert("requires_digest_confirmation".to_string(), json!(true));
        object.insert("execution_enabled".to_string(), json!(false));
        object.insert("safe_to_apply".to_string(), json!(false));
        object.insert("effects".to_string(), json!(plan.projections));
        object.insert(
            "projection_state".to_string(),
            json!(if plan.projections.is_empty() {
                "not_applicable"
            } else {
                "planned"
            }),
        );
        object.insert("conflicts".to_string(), json!(conflicts));
        object.insert("risks".to_string(), json!(risks));
        object.insert("recovery".to_string(), json!({"rollback_supported": false}));
        object.insert(
            "guards".to_string(),
            json!({
                "root": canonical_root(&self.ctx.root)?,
                "registry_head": plan.source.registry_head,
                "skill": plan.skill,
                "source_digest": plan.source.tree_digest,
                "registry_checkpoint_digest": plan.registry.checkpoint_digest,
            }),
        );
        Ok((output, Meta::default()))
    }
}

fn resolve_projection_input_evidence(
    ctx: &crate::state::AppContext,
    snapshot: Option<&RegistrySnapshot>,
    effects: &[ProjectionEffectPlan],
) -> std::result::Result<Vec<ProjectionInputEvidence>, CommandFailure> {
    let records = snapshot.map(|snapshot| &snapshot.projections.projections);
    effects
        .iter()
        .map(|effect| {
            let Some(record) = records.and_then(|records| {
                records
                    .iter()
                    .find(|record| record.instance_id == effect.instance_id)
            }) else {
                let unmanaged_live_digest = effect.materialized_tree_digest.clone();
                return Ok(ProjectionInputEvidence {
                    instance_id: effect.instance_id.clone(),
                    method: effect.method.clone(),
                    materialized_path: effect.materialized_path.clone(),
                    baseline_revision: None,
                    baseline_tree_digest: None,
                    live_tree_digest: unmanaged_live_digest.clone(),
                    state: if unmanaged_live_digest.is_some() {
                        ProjectionInputState::MetadataMismatch
                    } else {
                        ProjectionInputState::Untracked
                    },
                    issue: Some(if unmanaged_live_digest.is_some() {
                        "unmanaged_live_path_without_projection_record".to_string()
                    } else {
                        "projection_record_missing".to_string()
                    }),
                });
            };
            if record.method.as_str() != effect.method
                || record.materialized_path != effect.materialized_path
            {
                return Ok(ProjectionInputEvidence {
                    instance_id: effect.instance_id.clone(),
                    method: effect.method.clone(),
                    materialized_path: effect.materialized_path.clone(),
                    baseline_revision: Some(record.last_applied_rev.clone()),
                    baseline_tree_digest: None,
                    live_tree_digest: None,
                    state: ProjectionInputState::MetadataMismatch,
                    issue: Some("projection_record_does_not_match_active_rule".to_string()),
                });
            }
            projection_input_evidence(ctx, record)
        })
        .collect()
}

fn selected_input(
    args: &PlanConvergeArgs,
    evidence: &[ProjectionInputEvidence],
    source_digest: &str,
) -> std::result::Result<(String, Option<PathBuf>), CommandFailure> {
    if !args.from_projection {
        return Ok((source_digest.to_string(), None));
    }
    let instance = args.instance.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--from-projection requires exactly one --instance",
        )
    })?;
    let selected = evidence
        .iter()
        .find(|item| item.instance_id == instance)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ProjectionConflict,
                format!("projection input instance '{instance}' has no input evidence"),
            )
        })?;
    if !matches!(selected.method.as_str(), "copy" | "materialize") {
        return Err(invalid_projection_input(selected));
    }
    if !selected.state.is_usable_input()
        && selected.state != ProjectionInputState::BaselineUnavailable
    {
        return Err(invalid_projection_input(selected));
    }
    let digest = selected
        .live_tree_digest
        .clone()
        .ok_or_else(|| invalid_projection_input(selected))?;
    Ok((digest, Some(PathBuf::from(&selected.materialized_path))))
}

fn invalid_projection_input(evidence: &ProjectionInputEvidence) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!(
            "projection input instance '{}' is not a usable copy or materialize directory",
            evidence.instance_id
        ),
    );
    failure.details = json!({ "input_evidence": evidence });
    failure
}

fn resolve_input_conflicts(
    source_dirty_paths: &[String],
    projections: &[ProjectionInputEvidence],
    preflight: &crate::core::convergence::ConvergencePreflightEvidence,
    direction: &ConvergenceInputDirection,
    selected_instance: Option<&str>,
    selected_source_drift: bool,
) -> Vec<ConvergenceInputConflict> {
    let dirty = projections
        .iter()
        .filter(|projection| projection.state.is_dirty())
        .collect::<Vec<_>>();
    let mut conflicts = Vec::new();
    if !source_dirty_paths.is_empty() && !dirty.is_empty() {
        conflicts.push(ConvergenceInputConflict {
            code: "SOURCE_PROJECTION_DIRTY_CONFLICT".to_string(),
            message: "canonical source and one or more projections are both dirty".to_string(),
            evidence: json!({
                "source_dirty_paths": source_dirty_paths,
                "dirty_projections": dirty,
            }),
        });
    }
    let dirty_digests = dirty
        .iter()
        .filter_map(|projection| projection.live_tree_digest.as_deref())
        .collect::<BTreeSet<_>>();
    if dirty_digests.len() > 1 {
        conflicts.push(ConvergenceInputConflict {
            code: "DIVERGENT_PROJECTION_INPUTS".to_string(),
            message: "dirty projections contain divergent content".to_string(),
            evidence: json!({ "dirty_projections": dirty }),
        });
    }
    if dirty.len() > 1 && selected_instance.is_none() {
        conflicts.push(ConvergenceInputConflict {
            code: "MULTIPLE_DIRTY_PROJECTION_INPUTS".to_string(),
            message: "multiple dirty projections require an explicit instance selection"
                .to_string(),
            evidence: json!({ "dirty_projections": dirty }),
        });
    }
    if *direction == ConvergenceInputDirection::Projection && selected_source_drift {
        let selected = selected_instance
            .and_then(|instance| projections.iter().find(|item| item.instance_id == instance));
        conflicts.push(ConvergenceInputConflict {
            code: "STALE_PROJECTION_INPUT".to_string(),
            message: "canonical source changed since the selected projection baseline".to_string(),
            evidence: json!({
                "projection": selected,
            }),
        });
    }
    for projection in projections
        .iter()
        .filter(|projection| projection.state.is_fail_closed())
    {
        conflicts.push(ConvergenceInputConflict {
            code: "PROJECTION_EVIDENCE_UNAVAILABLE".to_string(),
            message: format!(
                "projection '{}' cannot be classified safely",
                projection.instance_id
            ),
            evidence: json!({ "projection": projection }),
        });
    }
    if !preflight.mutation_allowed {
        conflicts.push(ConvergenceInputConflict {
            code: "SOURCE_PREFLIGHT_BLOCKED".to_string(),
            message: "selected source input did not pass required preflight gates".to_string(),
            evidence: json!({
                "checks": preflight.checks,
                "regression_ids": preflight.regression_ids,
            }),
        });
    }
    conflicts
}

fn resolve_projection_effects(
    ctx: &AppContext,
    snapshot: Option<&RegistrySnapshot>,
    args: &PlanConvergeArgs,
    workspace: Option<&Path>,
    source_digest: &str,
) -> std::result::Result<Vec<ProjectionEffectPlan>, CommandFailure> {
    let Some(snapshot) = snapshot else {
        return Ok(Vec::new());
    };
    let mut effects = BTreeMap::new();
    for rule in snapshot
        .rules
        .rules
        .iter()
        .filter(|rule| rule.skill_id == args.skill)
    {
        let binding = snapshot.binding(&rule.binding_id).ok_or_else(|| {
            corrupt_state(format!(
                "active rule for skill '{}' references missing binding '{}'",
                args.skill, rule.binding_id
            ))
        })?;
        if !binding.active
            || args
                .agent
                .is_some_and(|agent| binding.agent.as_str() != agent.as_str())
            || args
                .profile
                .as_ref()
                .is_some_and(|profile| binding.profile_id != *profile)
            || workspace.is_some_and(|path| {
                !workspace_matches(
                    binding.workspace_matcher.kind.as_str(),
                    &binding.workspace_matcher.value,
                    path,
                )
            })
        {
            continue;
        }
        let target = snapshot.target(&rule.target_id).ok_or_else(|| {
            corrupt_state(format!(
                "active rule for skill '{}' references missing target '{}'",
                args.skill, rule.target_id
            ))
        })?;
        if target.agent != binding.agent {
            return Err(corrupt_state(format!(
                "binding '{}' and target '{}' have different agents",
                binding.binding_id, target.target_id
            )));
        }
        let instance_id =
            projection_instance_id(&args.skill, &binding.binding_id, &target.target_id);
        let existing = snapshot.projections.projections.iter().find(|projection| {
            projection.instance_id == instance_id
                && projection.binding_id.as_deref() == Some(binding.binding_id.as_str())
        });
        let materialized_path = PathBuf::from(&target.path).join(&args.skill);
        let observed_projection = existing
            .cloned()
            .unwrap_or_else(|| RegistryProjectionInstance {
                instance_id: instance_id.clone(),
                skill_id: args.skill.clone(),
                binding_id: Some(binding.binding_id.clone()),
                target_id: target.target_id.clone(),
                materialized_path: materialized_path.display().to_string(),
                method: rule.method,
                last_applied_rev: String::new(),
                health: crate::core::vocab::Health::Missing,
                observed_drift: None,
                source_tree_digest: None,
                materialized_tree_digest: None,
                last_observed_at: None,
                last_observed_error: None,
                updated_at: None,
            });
        let observation = observe_projection(ctx, &observed_projection);
        let effect = ProjectionEffectPlan {
            instance_id: instance_id.clone(),
            binding_id: binding.binding_id.clone(),
            target_id: target.target_id.clone(),
            agent: binding.agent.to_string(),
            profile: binding.profile_id.clone(),
            method: rule.method.as_str().to_string(),
            ownership: target.ownership.as_str().to_string(),
            materialized_path: materialized_path.display().to_string(),
            source_tree_digest: source_digest.to_string(),
            materialized_tree_digest: observation.materialized_tree_digest,
            effect: if existing.is_some() {
                "refresh".to_string()
            } else {
                "create".to_string()
            },
        };
        effects.insert(instance_id, effect);
    }
    Ok(effects.into_values().collect())
}

fn validate_projection_input(
    args: &PlanConvergeArgs,
    effects: &[ProjectionEffectPlan],
) -> std::result::Result<(), CommandFailure> {
    if !args.from_projection {
        return Ok(());
    }
    let instance = args.instance.as_deref().ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::ArgInvalid,
            "--from-projection requires exactly one --instance",
        )
    })?;
    if let Some(effect) = effects.iter().find(|effect| effect.instance_id == instance) {
        if effect.materialized_tree_digest.is_some() {
            return Ok(());
        }
        return Err(CommandFailure::new(
            ErrorCode::ProjectionConflict,
            format!(
                "projection input instance '{}' has no readable materialized bytes",
                instance
            ),
        ));
    }
    Err(CommandFailure::new(
        ErrorCode::ProjectionConflict,
        format!(
            "projection input instance '{}' is not in the selected active projection set",
            instance
        ),
    ))
}

fn registry_guard(
    snapshot: Option<&RegistrySnapshot>,
) -> std::result::Result<RegistryGuard, CommandFailure> {
    let Some(snapshot) = snapshot else {
        return Ok(RegistryGuard {
            initialized: false,
            checkpoint_digest: None,
            checkpoint_updated_at: None,
        });
    };
    Ok(RegistryGuard {
        initialized: true,
        checkpoint_digest: Some(digest_value(&snapshot.checkpoint)?),
        checkpoint_updated_at: Some(snapshot.checkpoint.updated_at.to_rfc3339()),
    })
}

fn digest_value(value: &impl Serialize) -> std::result::Result<String, CommandFailure> {
    let bytes = serde_json::to_vec(value).map_err(map_io)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn corrupt_state(message: String) -> CommandFailure {
    CommandFailure::new(ErrorCode::StateCorrupt, message)
}
