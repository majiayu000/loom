use std::collections::BTreeSet;
use std::fs;

use serde_json::{Value, json};

use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::{map_io, validate_skill_name};
use super::super::skill_deps::{SkillDependencyReport, skill_dependency_report};
use super::super::skill_eval_harness::cases::{
    HarnessJsonlRecord, HarnessTriggerCase, read_harness_jsonl,
};
use super::super::skill_inventory::tokenize;

#[derive(Default)]
pub(super) struct RankingEvidence {
    pub(super) score_delta: i64,
    pub(super) reasons: Vec<String>,
    pub(super) risks: Vec<String>,
    pub(super) warnings: Vec<String>,
    pub(super) score_inputs: Vec<Value>,
}

#[derive(Default)]
pub(super) struct EvalSummaryEvidence {
    pub(super) persisted: bool,
    pub(super) failed: u64,
    pub(super) trigger_precision: Option<f64>,
    pub(super) trigger_recall: Option<f64>,
    pub(super) baseline_delta: Option<f64>,
}

pub(super) fn ranking_evidence(
    ctx: &AppContext,
    skill_id: &str,
    skill: &Value,
    agent: Option<&str>,
    task: Option<&str>,
) -> std::result::Result<RankingEvidence, CommandFailure> {
    let mut evidence = RankingEvidence::default();
    if validate_skill_name(skill_id).is_err() {
        evidence
            .warnings
            .push("non-portable skill id; ranking evidence unavailable".to_string());
        return Ok(evidence);
    }
    add_dependency_evidence(ctx, skill_id, skill, agent, &mut evidence)?;
    add_eval_evidence(
        ctx,
        skill_id,
        agent,
        task.unwrap_or_default(),
        &mut evidence,
    )?;
    Ok(evidence)
}

fn add_dependency_evidence(
    ctx: &AppContext,
    skill_id: &str,
    skill: &Value,
    agent: Option<&str>,
    evidence: &mut RankingEvidence,
) -> std::result::Result<(), CommandFailure> {
    let Some(report) = dependency_report_for_skill(ctx, skill_id, skill, agent)? else {
        return Ok(());
    };
    if report.sources.is_empty() {
        evidence
            .warnings
            .push("no dependency metadata recorded".to_string());
        return Ok(());
    }
    match report.status.as_str() {
        "ready" => {
            evidence.score_delta += 2;
            evidence.reasons.push("dependencies ready".to_string());
            evidence.score_inputs.push(json!({
                "field": "dependency_readiness",
                "status": report.status,
                "weight": 2,
            }));
        }
        "unknown" => {
            evidence.score_delta -= 3;
            evidence
                .warnings
                .push("dependency readiness unknown".to_string());
            evidence.score_inputs.push(json!({
                "field": "dependency_readiness",
                "status": report.status,
                "weight": -3,
            }));
        }
        _ => {
            evidence.score_delta -= 8;
            evidence
                .risks
                .push(format!("dependency readiness {}", report.status));
            for finding in report.findings.iter().take(3) {
                evidence
                    .risks
                    .push(format!("dependency {}: {}", finding.id, finding.message));
            }
            evidence.score_inputs.push(json!({
                "field": "dependency_readiness",
                "status": report.status,
                "weight": -8,
            }));
        }
    }
    Ok(())
}

fn add_eval_evidence(
    ctx: &AppContext,
    skill_id: &str,
    agent: Option<&str>,
    task: &str,
    evidence: &mut RankingEvidence,
) -> std::result::Result<(), CommandFailure> {
    let summary = latest_eval_summary(ctx, skill_id, agent)?;
    if summary.persisted {
        if summary.failed > 0 {
            evidence.score_delta -= 8;
            evidence.risks.push(format!(
                "eval evidence has {} failing case(s)",
                summary.failed
            ));
            evidence.score_inputs.push(json!({
                "field": "eval_evidence",
                "metric": "failed",
                "weight": -8,
            }));
        }
        if let Some(delta) = summary.baseline_delta {
            if delta > 0.0 {
                let weight = (delta * 10.0).round().clamp(1.0, 8.0) as i64;
                evidence.score_delta += weight;
                evidence
                    .reasons
                    .push(format!("eval baseline delta {:.2}", delta));
                evidence.score_inputs.push(json!({
                    "field": "eval_evidence",
                    "metric": "baseline_delta",
                    "value": delta,
                    "weight": weight,
                }));
            } else if delta < 0.0 {
                let weight = -((-delta * 10.0).round().clamp(1.0, 8.0) as i64);
                evidence.score_delta += weight;
                evidence
                    .risks
                    .push(format!("eval baseline delta {:.2}", delta));
                evidence.score_inputs.push(json!({
                    "field": "eval_evidence",
                    "metric": "baseline_delta",
                    "value": delta,
                    "weight": weight,
                }));
            }
        }
        if let Some(recall) = summary.trigger_recall
            && recall >= 0.75
        {
            evidence.score_delta += 2;
            evidence
                .reasons
                .push(format!("trigger eval recall {:.2}", recall));
            evidence.score_inputs.push(json!({
                "field": "trigger_eval",
                "metric": "recall",
                "value": recall,
                "weight": 2,
            }));
        }
        if let Some(precision) = summary.trigger_precision
            && precision >= 0.75
        {
            evidence.score_delta += 2;
            evidence
                .reasons
                .push(format!("trigger eval precision {:.2}", precision));
            evidence.score_inputs.push(json!({
                "field": "trigger_eval",
                "metric": "precision",
                "value": precision,
                "weight": 2,
            }));
        }
    } else {
        evidence.warnings.push("no eval evidence".to_string());
    }

    let trigger_match = trigger_fixture_match(ctx, skill_id, task)?;
    if trigger_match.positive_matches > 0 {
        let weight = (trigger_match.positive_matches as i64 * 4).min(8);
        evidence.score_delta += weight;
        evidence
            .reasons
            .push("positive trigger fixture matches task".to_string());
        evidence.score_inputs.push(json!({
            "field": "positive_triggers",
            "matches": trigger_match.positive_matches,
            "weight": weight,
        }));
    }
    if trigger_match.negative_matches > 0 {
        let weight = -((trigger_match.negative_matches as i64 * 8).min(16));
        evidence.score_delta += weight;
        evidence
            .risks
            .push("negative trigger fixture matches task".to_string());
        evidence.score_inputs.push(json!({
            "field": "negative_triggers",
            "matches": trigger_match.negative_matches,
            "weight": weight,
        }));
    }
    Ok(())
}

pub(super) fn member_dependency_risk(
    ctx: &AppContext,
    skill_id: &str,
    agent: Option<&str>,
    skill: &Value,
) -> std::result::Result<Option<String>, CommandFailure> {
    let Some(report) = dependency_report_for_skill(ctx, skill_id, skill, agent)? else {
        return Ok(None);
    };
    if report.ready {
        return Ok(None);
    }
    Ok(Some(format!("dependency readiness {}", report.status)))
}

pub(super) fn dependency_report_for_skill(
    ctx: &AppContext,
    skill_id: &str,
    skill: &Value,
    agent: Option<&str>,
) -> std::result::Result<Option<SkillDependencyReport>, CommandFailure> {
    if skill["source_status"].as_str() != Some("present") {
        return Ok(None);
    }
    if validate_skill_name(skill_id).is_err() {
        return Ok(None);
    }
    skill_dependency_report(ctx, skill_id, agent, None).map(Some)
}

pub(super) fn dependency_tools(report: Option<&SkillDependencyReport>) -> Vec<String> {
    report
        .map(|report| {
            report
                .dependencies
                .tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

struct TriggerFixtureMatch {
    positive_matches: usize,
    negative_matches: usize,
}

fn trigger_fixture_match(
    ctx: &AppContext,
    skill_id: &str,
    task: &str,
) -> std::result::Result<TriggerFixtureMatch, CommandFailure> {
    let records = trigger_fixture_records(ctx, skill_id)?;
    let mut positive_matches = 0;
    let mut negative_matches = 0;
    for record in records {
        let Some(expected) = record.value.expected_trigger() else {
            continue;
        };
        let Some(prompt) = record.value.prompt.as_deref() else {
            continue;
        };
        if !text_match_is_meaningful(task, prompt) {
            continue;
        }
        if expected {
            positive_matches += 1;
        } else {
            negative_matches += 1;
        }
    }
    Ok(TriggerFixtureMatch {
        positive_matches,
        negative_matches,
    })
}

pub(super) fn trigger_fixture_prompts(
    ctx: &AppContext,
    skill_id: &str,
) -> std::result::Result<Vec<String>, CommandFailure> {
    Ok(trigger_fixture_records(ctx, skill_id)?
        .into_iter()
        .filter(|record| record.value.expected_trigger() == Some(true))
        .filter_map(|record| record.value.prompt)
        .collect())
}

fn trigger_fixture_records(
    ctx: &AppContext,
    skill_id: &str,
) -> std::result::Result<Vec<HarnessJsonlRecord<HarnessTriggerCase>>, CommandFailure> {
    read_harness_jsonl::<HarnessTriggerCase>(&ctx.skill_path(skill_id).join("evals/triggers.jsonl"))
}

pub(super) fn latest_eval_summary(
    ctx: &AppContext,
    skill_id: &str,
    agent: Option<&str>,
) -> std::result::Result<EvalSummaryEvidence, CommandFailure> {
    let mut evidence = EvalSummaryEvidence::default();
    for mode in ["run", "trigger", "compare"] {
        let path = ctx
            .state_dir
            .join("registry/evals")
            .join(skill_id)
            .join(format!("{mode}-latest.json"));
        if !path.is_file() {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(map_io)?;
        let parsed: Value = serde_json::from_str(&raw).map_err(|err| {
            CommandFailure::new(
                ErrorCode::StateCorrupt,
                format!("failed to parse {}: {}", path.display(), err),
            )
        })?;
        if !eval_report_matches_agent(&parsed, agent) {
            continue;
        }
        evidence.persisted = true;
        let summary = &parsed["summary"];
        evidence.failed += summary["failed"].as_u64().unwrap_or(0);
        evidence.trigger_precision = max_option(
            evidence.trigger_precision,
            summary["trigger_precision"].as_f64(),
        );
        evidence.trigger_recall =
            max_option(evidence.trigger_recall, summary["trigger_recall"].as_f64());
        evidence.baseline_delta = max_option(
            evidence.baseline_delta,
            summary["delta"]
                .as_f64()
                .or_else(|| summary["baseline_delta"].as_f64()),
        );
    }
    Ok(evidence)
}

fn eval_report_matches_agent(report: &Value, agent: Option<&str>) -> bool {
    let Some(agent) = agent else {
        return true;
    };
    if report["agent"].as_str() == Some(agent) {
        return true;
    }
    let has_agent_metadata = report["agent"].as_str().is_some()
        || report["matrix"].as_array().is_some()
        || report["runs"].as_array().is_some();
    if report["matrix"]
        .as_array()
        .is_some_and(|matrix| matrix.iter().any(|value| value.as_str() == Some(agent)))
    {
        return true;
    }
    if report["runs"]
        .as_array()
        .is_some_and(|runs| runs.iter().any(|run| run["agent"].as_str() == Some(agent)))
    {
        return true;
    }
    !has_agent_metadata
}

fn max_option(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn text_match_is_meaningful(task: &str, prompt: &str) -> bool {
    let task_tokens = meaningful_tokens(task);
    let prompt_tokens = meaningful_tokens(prompt);
    if task_tokens.is_empty() || prompt_tokens.is_empty() {
        return false;
    }
    let overlap = task_tokens
        .iter()
        .filter(|token| prompt_tokens.contains(*token))
        .count();
    overlap >= 2 || normalized_contains(task, prompt) || normalized_contains(prompt, task)
}

fn meaningful_tokens(value: &str) -> BTreeSet<String> {
    tokenize(value)
        .into_iter()
        .filter(|token| token.len() >= 3)
        .filter(|token| {
            !matches!(
                token.as_str(),
                "and" | "for" | "the" | "this" | "that" | "use" | "when" | "with"
            )
        })
        .collect()
}

fn normalized_contains(left: &str, right: &str) -> bool {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    !left.is_empty() && !right.is_empty() && left.contains(&right)
}
