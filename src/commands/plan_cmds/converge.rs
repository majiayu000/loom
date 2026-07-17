use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::PlanConvergeArgs;
use crate::core::convergence::{
    ConvergenceAxis, ConvergenceInputDirection, ConvergenceSelectors, ProjectionEffectPlan,
    RegistryGuard, RemotePolicy, SkillConvergencePlan, SourceGuard, VisibilityRequirement,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::sha256::{Sha256, to_hex};
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::super::agent_cmds::planning_helpers::{normalize_path, workspace_matches};
use super::super::helpers::{
    map_arg, map_git, map_io, map_registry_state, projection_instance_id, validate_skill_name,
};
use super::super::provenance::{materialized_tree_digest, skill_tree_digest};
use super::super::skill_policy::evaluate_skill_policy;
use super::super::{App, CommandFailure};
use super::{
    PLAN_PROTOCOL_VERSION, PLAN_SCHEMA_VERSION, canonical_root, policy_risks, required_approvals,
};

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
        let policy = evaluate_skill_policy(&self.ctx, &args.skill, "safe-capture")?;
        let approvals = required_approvals(&policy);
        let projections = resolve_projection_effects(
            snapshot.as_ref(),
            args,
            workspace.as_deref(),
            &source_digest,
        )?;

        validate_projection_input(args, &projections)?;
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
                direction: if args.from_projection {
                    ConvergenceInputDirection::Projection
                } else {
                    ConvergenceInputDirection::Source
                },
                registry_head,
                tree_digest: source_digest,
                input_instance: args.instance.clone(),
            },
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

        let mut conflicts = Vec::new();
        if args.require_runtime && plan.projections.is_empty() {
            conflicts.push(json!({
                "code": "RUNTIME_PROJECTION_REQUIRED",
                "message": "--require-runtime resolved no active projection",
            }));
        }
        let mut risks = policy_risks(&policy);
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
        object.insert("schema_version".to_string(), json!(PLAN_SCHEMA_VERSION));
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

fn resolve_projection_effects(
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
        let materialized_digest = match fs::symlink_metadata(&materialized_path) {
            Ok(_) => Some(materialized_tree_digest(&materialized_path).map_err(map_io)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(CommandFailure::new(
                    ErrorCode::IoError,
                    format!(
                        "failed to inspect projection path '{}': {}",
                        materialized_path.display(),
                        err
                    ),
                ));
            }
        };
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
            materialized_tree_digest: materialized_digest,
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
    if effects.iter().any(|effect| effect.instance_id == instance) {
        return Ok(());
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
