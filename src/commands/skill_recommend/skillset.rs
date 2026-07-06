use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::state::AppContext;

use super::super::helpers::map_registry_state;
use super::super::skill_inventory::tokenize;
use super::super::telemetry::SkillTelemetryEvidenceCache;
use super::super::{CommandFailure, build_skill_read_model};
use super::evidence::member_dependency_risk;
use super::{RecommendationContext, activation_safety_risk};

pub(super) fn skillset_recommendations(
    ctx: &AppContext,
    task: &str,
    request: RecommendationContext<'_>,
    skill_results: &[Value],
    skillsets: &Value,
    telemetry_cache: &mut SkillTelemetryEvidenceCache,
) -> std::result::Result<Vec<Value>, CommandFailure> {
    let inventory = build_skill_read_model(ctx).map_err(map_registry_state)?;
    let inventory = inventory
        .skills
        .into_iter()
        .filter_map(|skill| {
            skill["skill_id"]
                .as_str()
                .map(|id| (id.to_string(), skill.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let skill_scores = skill_results
        .iter()
        .filter_map(|result| {
            Some((
                result["id"].as_str()?.to_string(),
                result["score"].as_i64().unwrap_or_default(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let tokens = tokenize(task);
    let mut out = Vec::new();
    for skillset in skillsets["skillsets"].as_array().into_iter().flatten() {
        let Some(id) = skillset["id"].as_str() else {
            continue;
        };
        let mut score = lexical_score_text(id, &tokens)
            + lexical_score_text(
                skillset["description"].as_str().unwrap_or_default(),
                &tokens,
            );
        let mut risks = Vec::new();
        let mut warnings = Vec::new();
        let mut reasons = Vec::new();
        let mut required_safe = true;
        let mut member_commands = Vec::new();
        for member in skillset["members"].as_array().into_iter().flatten() {
            let Some(skill_id) = member["skill_id"].as_str() else {
                continue;
            };
            let required = member["required"].as_bool().unwrap_or(true);
            let telemetry_risky =
                member_telemetry_risky(ctx, request, task, skill_id, telemetry_cache);
            let mut member_score = *skill_scores.get(skill_id).unwrap_or(&0);
            if telemetry_risky {
                member_score -= 8;
            }
            match inventory.get(skill_id) {
                Some(skill) => {
                    let member_kind = if required { "required" } else { "optional" };
                    if telemetry_risky {
                        if required {
                            required_safe = false;
                            risks.push(format!("{member_kind} member '{skill_id}' telemetry risk"));
                        } else {
                            warnings
                                .push(format!("{member_kind} member '{skill_id}' telemetry risk"));
                        }
                    }
                    score += member_score / 2;
                    if member_score > 0 {
                        reasons.push(format!("member '{skill_id}' matched"));
                    }
                    if skill["quarantined"].as_bool() == Some(true) {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' quarantined"));
                    } else if skill["trust"].as_str() == Some("blocked") {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' trust blocked"));
                    } else if skill["source_status"].as_str() != Some("present") {
                        if required {
                            required_safe = false;
                            risks.push(format!("required member '{skill_id}' source missing"));
                        } else {
                            warnings.push(format!("optional member '{skill_id}' source missing"));
                        }
                    } else if let Some(risk) =
                        activation_safety_risk(ctx, skill_id, request.policy_profile)?
                    {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' {risk}"));
                    } else if let Some(dependency_risk) =
                        member_dependency_risk(ctx, skill_id, request.agent, skill)?
                    {
                        if required {
                            required_safe = false;
                            score -= 8;
                            risks.push(format!("required member '{skill_id}' {dependency_risk}"));
                        } else {
                            warnings
                                .push(format!("optional member '{skill_id}' {dependency_risk}"));
                        }
                    } else if !skill["warnings"].as_array().is_none_or(Vec::is_empty) {
                        required_safe = false;
                        risks.push(format!("{member_kind} member '{skill_id}' warnings"));
                    } else if !telemetry_risky && let Some(agent) = request.agent {
                        member_commands.push(format!(
                            "loom --json skill activate {skill_id} --agent {agent} --dry-run"
                        ));
                    }
                }
                None => {
                    score += member_score / 2;
                    if member_score > 0 {
                        reasons.push(format!("member '{skill_id}' matched"));
                    }
                    if required {
                        required_safe = false;
                        risks.push(format!("required member '{skill_id}' missing"));
                    } else {
                        warnings.push(format!("optional member '{skill_id}' missing"));
                    }
                }
            }
        }
        if score == 0 {
            continue;
        }
        if reasons.is_empty() {
            reasons.push("skillset text matched".to_string());
        }
        warnings.push("skillset activation unavailable".to_string());
        let can_activate_members = required_safe && risks.is_empty() && request.agent.is_some();
        out.push(json!({
            "kind": "skillset",
            "id": id,
            "score": score.max(0),
            "mode": request.mode,
            "score_inputs": {
                "matched_fields": ["skillset", "members"],
            },
            "reasons": reasons,
            "risks": risks,
            "warnings": warnings,
            "recommended_action": if can_activate_members { "activate" } else { "inspect" },
            "suggested_commands": if can_activate_members && !member_commands.is_empty() {
                member_commands
            } else {
                vec![format!("loom --json skillset show {id}")]
            },
        }));
    }
    Ok(out)
}

fn lexical_score_text(value: &str, tokens: &[String]) -> i64 {
    let value = value.to_ascii_lowercase();
    tokens
        .iter()
        .filter(|token| value.contains(token.as_str()))
        .count() as i64
        * 4
}

fn member_telemetry_risky(
    ctx: &AppContext,
    request: RecommendationContext<'_>,
    task: &str,
    skill_id: &str,
    telemetry_cache: &mut SkillTelemetryEvidenceCache,
) -> bool {
    let Ok(telemetry) =
        telemetry_cache.evidence_for(ctx, skill_id, request.agent, request.workspace, Some(task))
    else {
        return false;
    };
    telemetry.enabled
        && telemetry.events > 0
        && (telemetry.errors > 0 || telemetry.feedback_rejected > 0)
}
