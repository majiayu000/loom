use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

use serde_json::{Value, json};

use crate::cli::{ActiveRecommendArgs, SkillSearchArgs};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::state_model::{REGISTRY_SCHEMA_VERSION, RegistryStatePaths, RegistryWorkspaceBinding};
use crate::types::ErrorCode;

use super::helpers::{
    map_io, map_registry_state, validate_non_empty, validate_policy_profile, validate_skill_name,
};
use super::skill_inventory::{SkillDiscoveryFilters, score_and_filter_skills};
use super::skill_recommend_active::{activation_plan_delta, active_view};
use super::skill_safety::evaluate_skill_safety_with_policy;
use super::telemetry::SkillTelemetryEvidenceCache;
use super::{App, CommandFailure, build_skill_read_model};

#[path = "skill_recommend/evidence.rs"]
mod evidence;
use evidence::ranking_evidence;

#[path = "skill_recommend/index.rs"]
mod index;

#[path = "skill_recommend/skillset.rs"]
mod skillset;
use skillset::skillset_recommendations;

impl App {
    pub fn cmd_skill_search(
        &self,
        args: &SkillSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_non_empty("query", args.query.trim())?;
        let mut warnings = Vec::new();
        let mode = if args.semantic {
            warnings.push("semantic provider not configured".to_string());
            "semantic-disabled"
        } else {
            "lexical"
        };
        let policy_context = recommendation_policy_context(&self.ctx, args)?;
        let model = build_skill_read_model(&self.ctx).map_err(map_registry_state)?;
        let mut results = score_and_filter_skills(
            &model.skills,
            &args.query,
            SkillDiscoveryFilters {
                agent: args.agent.as_deref(),
                profile: args.profile.as_deref(),
                status: args.status.as_deref(),
                trust: args.trust.as_deref(),
                workspace: args.workspace.as_deref(),
            },
            args.for_task || args.workspace.is_some(),
        );
        if args.active {
            results.retain(|result| {
                result["skill"]["projection_summary"]["count"]
                    .as_u64()
                    .unwrap_or_default()
                    > 0
            });
        }
        warnings.extend(model.warnings);
        let policy_profile = policy_context["policy_profile"]
            .as_str()
            .unwrap_or("safe-capture")
            .to_string();
        let recommendation_context = RecommendationContext {
            agent: args.agent.as_deref(),
            workspace: args.workspace.as_deref(),
            mode,
            policy_profile: &policy_profile,
        };
        let mut telemetry_cache = SkillTelemetryEvidenceCache::default();
        let adjusted_task_results = if args.for_task {
            Some(evidence_adjusted_skill_results(
                &self.ctx,
                &args.query,
                recommendation_context,
                &results,
                &mut telemetry_cache,
            )?)
        } else {
            None
        };
        let selected = adjusted_task_results
            .as_ref()
            .and_then(|results| results.first().cloned())
            .or_else(|| results.first().cloned());
        let candidates = adjusted_task_results
            .as_ref()
            .cloned()
            .unwrap_or_else(|| results.clone());
        let mut payload = json!({
            "query": args.query,
            "mode": mode,
            "for_task": args.for_task,
            "filters": {
                "agent": args.agent,
                "profile": args.profile,
                "status": args.status,
                "trust": args.trust,
                "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                "binding": args.binding,
                "policy_profile": args.policy_profile,
                "active": args.active,
            },
            "policy_context": policy_context,
            "count": results.len(),
            "results": results,
        });
        if args.for_task {
            payload["task_description"] = json!(args.query);
            payload["strategy"] = json!({
                "type": if args.semantic {
                    "semantic_disabled_lexical"
                } else {
                    "deterministic_lexical"
                },
                "mode": mode,
                "llm_invoked": false,
                "tie_break": "score_desc_then_skill_id_asc",
            });
            payload["selected"] = selected.unwrap_or(Value::Null);
            payload["candidates"] = json!(candidates);
        }
        if args.explain {
            let skillsets = load_skillsets_value(&self.ctx)?;
            let recommendation_skill_results = score_and_filter_skills(
                &model.skills,
                &args.query,
                SkillDiscoveryFilters {
                    agent: None,
                    profile: None,
                    status: None,
                    trust: None,
                    workspace: args.workspace.as_deref(),
                },
                true,
            );
            let recommendations = recommendation_results(
                &self.ctx,
                &args.query,
                recommendation_context,
                &recommendation_skill_results,
                &skillsets,
                &mut telemetry_cache,
            )?;
            payload["recommendations"] = json!({
                "task_description": args.query,
                "mode": mode,
                "filters": {
                    "agent": args.agent,
                    "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                    "binding": args.binding,
                    "policy_profile": args.policy_profile,
                },
                "count": recommendations.len(),
                "results": recommendations,
            });
            payload["explain"] = json!({
                "score_inputs": true,
                "skillsets": true,
                "safety_risks": true,
            });
        }
        Ok((
            payload,
            Meta {
                warnings,
                ..Meta::default()
            },
        ))
    }

    pub fn cmd_skill_recommend(
        &self,
        args: &SkillSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let mut args = args.clone();
        args.for_task = false;
        args.explain = true;
        self.cmd_skill_search(&args)
    }

    pub fn cmd_skill_resolve(
        &self,
        args: &SkillSearchArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let mut args = args.clone();
        args.for_task = true;
        self.cmd_skill_search(&args)
    }

    pub fn cmd_active_recommend(
        &self,
        args: &ActiveRecommendArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_non_empty("task_description", args.task_description.trim())?;
        validate_non_empty("agent", args.agent.trim())?;
        validate_active_agent(args.agent.trim())?;
        for skill in &args.desired_skills {
            validate_skill_name(skill).map_err(super::helpers::map_arg)?;
        }
        let model = build_skill_read_model(&self.ctx).map_err(map_registry_state)?;
        let skill_results = score_and_filter_skills(
            &model.skills,
            &args.task_description,
            SkillDiscoveryFilters {
                agent: None,
                profile: None,
                status: None,
                trust: None,
                workspace: args.workspace.as_deref(),
            },
            true,
        );
        let skillsets = load_skillsets_value(&self.ctx)?;
        let recommendation_context = RecommendationContext {
            agent: Some(args.agent.trim()),
            workspace: args.workspace.as_deref(),
            mode: "lexical",
            policy_profile: "safe-capture",
        };
        let mut telemetry_cache = SkillTelemetryEvidenceCache::default();
        let recommend = recommendation_results(
            &self.ctx,
            &args.task_description,
            recommendation_context,
            &skill_results,
            &skillsets,
            &mut telemetry_cache,
        )?;
        let mut meta = Meta {
            warnings: model.warnings,
            ..Meta::default()
        };
        let mut desired = args.desired_skills.clone();
        if desired.is_empty() {
            desired.extend(
                recommend
                    .iter()
                    .filter(|result| result["kind"].as_str() == Some("skill"))
                    .filter(|result| result["risks"].as_array().is_none_or(Vec::is_empty))
                    .filter_map(|result| result["id"].as_str().map(str::to_string))
                    .take(3),
            );
        }
        let active_view = active_view(
            &self.ctx,
            &args.agent,
            args.workspace.as_deref(),
            args.binding.as_deref(),
        )?;
        let (add, keep, remove, risks) = activation_plan_delta(
            &self.ctx,
            &desired,
            &args.agent,
            args.workspace.as_deref(),
            &active_view,
        )?;
        if add.is_empty() && keep.is_empty() {
            meta.warnings.push("no activation candidates".to_string());
        }
        Ok((
            json!({
                "agent": args.agent,
                "workspace": args.workspace.as_ref().map(|path| path.display().to_string()),
                "task": args.task_description,
                "binding_id": args.binding,
                "dry_run": true,
                "plan": {
                    "add": add,
                    "keep": keep,
                    "remove": remove,
                },
                "risks": risks,
                "policy": {
                    "allowed": risks.is_empty(),
                    "mode": "dry-run-only",
                },
                "suggested_commands": add.iter().filter_map(|item| item["command"].as_str().map(str::to_string)).collect::<Vec<_>>(),
            }),
            meta,
        ))
    }
}

fn recommendation_policy_context(
    ctx: &AppContext,
    args: &SkillSearchArgs,
) -> std::result::Result<Value, CommandFailure> {
    if let Some(binding_id) = args.binding.as_deref() {
        validate_non_empty("binding", binding_id.trim())?;
    }
    if let Some(policy_profile) = args.policy_profile.as_deref() {
        validate_policy_profile(policy_profile)?;
    }
    let Some(binding_id) = args.binding.as_deref() else {
        return Ok(json!({
            "binding_id": Value::Null,
            "policy_profile": args.policy_profile,
            "source": if args.policy_profile.is_some() { "explicit" } else { "none" },
        }));
    };
    let paths = RegistryStatePaths::from_app_context(ctx);
    let snapshot = paths.maybe_load_snapshot().map_err(map_registry_state)?;
    let binding = snapshot
        .as_ref()
        .and_then(|snapshot| {
            snapshot
                .bindings
                .bindings
                .iter()
                .find(|binding| binding.binding_id == binding_id)
        })
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::BindingNotFound,
                format!("binding '{binding_id}' not found"),
            )
        })?;
    if let Some(agent) = args.agent.as_deref()
        && binding.agent != agent
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "binding '{}' is for agent '{}' not '{}'",
                binding.binding_id, binding.agent, agent
            ),
        ));
    }
    if let Some(profile) = args.profile.as_deref()
        && binding.profile_id != profile
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "binding '{}' uses profile '{}' not '{}'",
                binding.binding_id, binding.profile_id, profile
            ),
        ));
    }
    if let Some(policy_profile) = args.policy_profile.as_deref()
        && binding.policy_profile != policy_profile
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "binding '{}' uses policy profile '{}' not '{}'",
                binding.binding_id, binding.policy_profile, policy_profile
            ),
        ));
    }
    if let Some(workspace) = args.workspace.as_deref() {
        validate_recommend_workspace_path(workspace)?;
        if !recommend_binding_matches_workspace(binding, workspace) {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "binding '{}' does not match workspace '{}'",
                    binding.binding_id,
                    workspace.display()
                ),
            ));
        }
    }
    Ok(json!({
        "binding_id": binding.binding_id,
        "agent": binding.agent,
        "profile": binding.profile_id,
        "policy_profile": binding.policy_profile,
        "active": binding.active,
        "source": "binding",
    }))
}

fn validate_recommend_workspace_path(workspace: &Path) -> std::result::Result<(), CommandFailure> {
    if workspace
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "workspace '{}' must not contain parent directory components",
                workspace.display()
            ),
        ));
    }
    Ok(())
}

fn recommend_binding_matches_workspace(
    binding: &RegistryWorkspaceBinding,
    workspace: &Path,
) -> bool {
    let matcher = &binding.workspace_matcher;
    match matcher.kind.as_str() {
        "path_prefix" => workspace.starts_with(Path::new(&matcher.value)),
        "exact_path" => workspace == Path::new(&matcher.value),
        "name" => workspace
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == matcher.value),
        _ => false,
    }
}

#[derive(Clone, Copy)]
struct RecommendationContext<'a> {
    agent: Option<&'a str>,
    workspace: Option<&'a Path>,
    mode: &'a str,
    policy_profile: &'a str,
}

fn recommendation_results(
    ctx: &AppContext,
    task: &str,
    request: RecommendationContext<'_>,
    skill_search_results: &[Value],
    skillsets: &Value,
    telemetry_cache: &mut SkillTelemetryEvidenceCache,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    let mut results = Vec::new();
    for result in skill_search_results {
        if let Some(recommendation) =
            skill_recommendation(ctx, task, result, request, telemetry_cache)?
        {
            results.push(recommendation);
        }
    }
    results.extend(skillset_recommendations(
        ctx,
        task,
        request,
        skill_search_results,
        skillsets,
        telemetry_cache,
    )?);
    results.sort_by(|left, right| {
        let l = left["score"].as_i64().unwrap_or_default();
        let r = right["score"].as_i64().unwrap_or_default();
        r.cmp(&l)
            .then_with(|| {
                left["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["kind"].as_str().unwrap_or_default())
            })
            .then_with(|| {
                left["id"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["id"].as_str().unwrap_or_default())
            })
    });
    Ok(results)
}

fn evidence_adjusted_skill_results(
    ctx: &AppContext,
    task: &str,
    request: RecommendationContext<'_>,
    skill_results: &[Value],
    telemetry_cache: &mut SkillTelemetryEvidenceCache,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    let mut adjusted = Vec::new();
    for result in skill_results {
        let skill = &result["skill"];
        let Some(skill_id) = skill["skill_id"].as_str() else {
            continue;
        };
        if skill["quarantined"].as_bool() == Some(true) {
            continue;
        }
        let evidence = ranking_evidence(
            ctx,
            skill_id,
            skill,
            request.agent,
            request.workspace,
            Some(task),
            telemetry_cache,
        )?;
        let mut result = result.clone();
        let score = result["score"].as_i64().unwrap_or_default() + evidence.score_delta;
        result["score"] = json!(score.max(0));
        if let Some(inputs) = result["score_inputs"].as_array_mut() {
            inputs.extend(evidence.score_inputs);
        }
        result["recommendation_risks"] = json!(evidence.risks);
        result["recommendation_warnings"] = json!(evidence.warnings);
        adjusted.push(result);
    }
    adjusted.sort_by(|left, right| {
        let l = left["score"].as_i64().unwrap_or_default();
        let r = right["score"].as_i64().unwrap_or_default();
        r.cmp(&l).then_with(|| {
            left["skill"]["skill_id"]
                .as_str()
                .unwrap_or_default()
                .cmp(right["skill"]["skill_id"].as_str().unwrap_or_default())
        })
    });
    Ok(adjusted)
}

fn skill_recommendation(
    ctx: &AppContext,
    task: &str,
    result: &Value,
    request: RecommendationContext<'_>,
    telemetry_cache: &mut SkillTelemetryEvidenceCache,
) -> std::result::Result<Option<Value>, CommandFailure> {
    let skill = &result["skill"];
    let Some(skill_id) = skill["skill_id"].as_str() else {
        return Ok(None);
    };
    if skill["quarantined"].as_bool() == Some(true) {
        return Ok(None);
    }
    let mut reasons = vec!["lexical match".to_string()];
    let mut risks = Vec::new();
    let mut warnings = Vec::new();
    let mut score = result["score"].as_i64().unwrap_or_default();
    let mut score_inputs = result["score_inputs"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if skill["trust"].as_str().unwrap_or("unknown") == "unknown" {
        warnings.push("no trust metadata recorded".to_string());
    }
    if skill["trust"].as_str() == Some("blocked") {
        risks.push("trust blocked".to_string());
    }
    let skill_name_valid = validate_skill_name(skill_id).is_ok();
    if !skill_name_valid {
        risks.push("non-portable skill id".to_string());
    } else if skill["source_status"].as_str() != Some("present") {
        risks.push(format!(
            "source {}",
            skill["source_status"].as_str().unwrap_or("unknown")
        ));
    } else if let Some(risk) = activation_safety_risk(ctx, skill_id, request.policy_profile)? {
        risks.push(risk);
    }
    let evidence = ranking_evidence(
        ctx,
        skill_id,
        skill,
        request.agent,
        request.workspace,
        Some(task),
        telemetry_cache,
    )?;
    score += evidence.score_delta;
    reasons.extend(evidence.reasons);
    risks.extend(evidence.risks);
    warnings.extend(evidence.warnings);
    score_inputs.extend(evidence.score_inputs);
    if !skill["warnings"].as_array().is_none_or(Vec::is_empty) {
        risks.push("inventory warnings".to_string());
    }
    if request.agent.is_some() {
        reasons.push("agent match".to_string());
    }
    let can_activate = risks.is_empty() && request.agent.is_some();
    Ok(Some(json!({
        "kind": "skill",
        "id": skill_id,
        "score": score.max(0),
        "mode": request.mode,
        "score_inputs": score_inputs,
        "reasons": reasons,
        "risks": risks,
        "warnings": warnings,
        "recommended_action": if can_activate { "activate" } else { "inspect" },
        "suggested_commands": if can_activate {
            vec![format!("loom --json skill activate {skill_id} --agent {} --dry-run", request.agent.unwrap())]
        } else {
            vec![format!("loom --json skill inspect {skill_id}")]
        },
    })))
}

fn activation_safety_risk(
    ctx: &AppContext,
    skill_id: &str,
    policy_profile: &str,
) -> std::result::Result<Option<String>, CommandFailure> {
    let evaluation =
        evaluate_skill_safety_with_policy(ctx, skill_id, "activate", false, policy_profile)?;
    if evaluation.report.activation_allowed {
        Ok(None)
    } else {
        Ok(Some(format!("safety {}", evaluation.report.decision)))
    }
}

fn validate_active_agent(agent: &str) -> std::result::Result<(), CommandFailure> {
    match agent {
        "claude" | "codex" | "cursor" | "windsurf" | "cline" | "copilot" | "aider" | "opencode"
        | "gemini-cli" | "goose" => Ok(()),
        _ => Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("unsupported agent '{agent}'"),
        )),
    }
}

fn skillset_membership(skillsets: &Value) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for skillset in skillsets["skillsets"].as_array().into_iter().flatten() {
        let Some(skillset_id) = skillset["id"].as_str() else {
            continue;
        };
        for member in skillset["members"].as_array().into_iter().flatten() {
            if let Some(skill_id) = member["skill_id"].as_str() {
                out.entry(skill_id.to_string())
                    .or_default()
                    .insert(skillset_id.to_string());
            }
        }
    }
    out.into_iter()
        .map(|(skill, sets)| (skill, sets.into_iter().collect()))
        .collect()
}

fn load_skillsets_value(ctx: &AppContext) -> std::result::Result<Value, CommandFailure> {
    let path = ctx.root.join("state/registry/skillsets.json");
    if !path.exists() {
        return Ok(json!({ "schema_version": REGISTRY_SCHEMA_VERSION, "skillsets": [] }));
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", path.display(), err),
        )
    })?;
    if parsed["schema_version"].as_u64() != Some(REGISTRY_SCHEMA_VERSION as u64) {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "{} schema {} unsupported",
                path.display(),
                parsed["schema_version"]
            ),
        ));
    }
    Ok(parsed)
}
