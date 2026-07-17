use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::super::CommandFailure;
use super::super::model::TelemetryEvent;
use super::super::store::read_event_log;
use super::{
    AgentStats, ScanPlan, SkillSummary, SourcePlan, checked_field_add, checked_increment,
    checked_map_sum,
};

pub(super) fn coalesce_sources(plan: &mut ScanPlan) -> std::result::Result<(), CommandFailure> {
    let mut grouped = BTreeMap::<String, SourcePlan>::new();
    for source in std::mem::take(&mut plan.sources) {
        let key = source.source_key.clone();
        if let Some(existing) = grouped.get_mut(&key) {
            debug_assert_eq!(existing.agent, source.agent);
            debug_assert_eq!(existing.expected, source.expected);
            existing.source_guards.extend(source.source_guards);
            existing.drafts.extend(source.drafts);
            merge_source_stats(&mut existing.stats, source.stats)?;
            for (reason, count) in source.rejected_reasons {
                checked_field_add(
                    existing.rejected_reasons.entry(reason).or_default(),
                    count,
                    "rejected",
                )?;
            }
            let source_continuous = source.expected.is_some() && source.reset_reason.is_none();
            let existing_continuous =
                existing.expected.is_some() && existing.reset_reason.is_none();
            if (source_continuous, &source.authority) > (existing_continuous, &existing.authority) {
                existing.checkpoint = source.checkpoint;
                existing.authority = source.authority;
                existing.reset_reason = source.reset_reason;
            }
        } else {
            grouped.insert(key, source);
        }
    }
    for source in grouped.values_mut() {
        let mut event_ids = BTreeSet::new();
        source.drafts.retain(|draft| {
            draft
                .event_id_override
                .as_ref()
                .is_none_or(|event_id| event_ids.insert(event_id.clone()))
        });
        if let Some(reason) = source.reset_reason {
            checked_increment(&mut plan.reset_reasons, reason.as_str().to_string())?;
        }
        let stats = plan.stats.entry(source.agent).or_default();
        checked_field_add(
            &mut stats.scanned_events,
            source.stats.scanned_events,
            "scanned_events",
        )?;
        checked_field_add(
            &mut stats.window_skipped,
            source.stats.window_skipped,
            "window_skipped",
        )?;
        checked_field_add(&mut stats.malformed, source.stats.malformed, "malformed")?;
        checked_field_add(
            &mut stats.pending_partial,
            source.stats.pending_partial,
            "pending_partial",
        )?;
        checked_field_add(&mut stats.rejected, source.stats.rejected, "rejected")?;
        for (reason, count) in &source.rejected_reasons {
            checked_field_add(
                plan.rejected_reasons.entry(reason.clone()).or_default(),
                *count,
                "rejected",
            )?;
        }
    }
    plan.sources = grouped.into_values().collect();
    Ok(())
}

fn merge_source_stats(
    target: &mut AgentStats,
    source: AgentStats,
) -> std::result::Result<(), CommandFailure> {
    checked_field_add(
        &mut target.scanned_events,
        source.scanned_events,
        "scanned_events",
    )?;
    checked_field_add(
        &mut target.window_skipped,
        source.window_skipped,
        "window_skipped",
    )?;
    checked_field_add(&mut target.malformed, source.malformed, "malformed")?;
    checked_field_add(
        &mut target.pending_partial,
        source.pending_partial,
        "pending_partial",
    )?;
    checked_field_add(&mut target.rejected, source.rejected, "rejected")
}

pub(super) fn preview_dedupe(
    ctx: &AppContext,
    plan: &mut ScanPlan,
) -> std::result::Result<(), CommandFailure> {
    let mut ids = read_event_log(ctx)?
        .events
        .into_iter()
        .map(|entry| entry.event.event_id)
        .collect::<BTreeSet<_>>();
    plan.matched.clear();
    plan.unmatched.clear();
    for source in &plan.sources {
        for draft in &source.drafts {
            let Some(event_id) = draft.event_id_override.as_ref() else {
                continue;
            };
            let stats = plan.stats.entry(source.agent).or_default();
            if ids.insert(event_id.clone()) {
                checked_field_add(&mut stats.ingested, 1, "ingested")?;
                record_summary(
                    &mut plan.matched,
                    &mut plan.unmatched,
                    draft.skill_id.as_deref(),
                    draft.observed_skill_name.as_deref(),
                    source.agent.as_str(),
                )?;
            } else {
                checked_field_add(&mut stats.duplicates_skipped, 1, "duplicates_skipped")?;
            }
        }
    }
    Ok(())
}

pub(super) fn summarize_events(
    events: &[TelemetryEvent],
) -> std::result::Result<(SkillSummary, SkillSummary), CommandFailure> {
    let mut matched = BTreeMap::new();
    let mut unmatched = BTreeMap::new();
    for event in events {
        let agent = event.agent.as_deref().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "imported telemetry event is missing its agent",
            )
        })?;
        record_summary(
            &mut matched,
            &mut unmatched,
            event.skill_id.as_deref(),
            event.observed_skill_name.as_deref(),
            agent,
        )?;
    }
    Ok((matched, unmatched))
}

fn record_summary(
    matched: &mut SkillSummary,
    unmatched: &mut SkillSummary,
    skill_id: Option<&str>,
    observed_skill_name: Option<&str>,
    agent: &str,
) -> std::result::Result<(), CommandFailure> {
    if let Some(name) = skill_id {
        return checked_increment(matched, (name.to_string(), agent.to_string()));
    }
    if let Some(name) = observed_skill_name {
        return checked_increment(unmatched, (name.to_string(), agent.to_string()));
    }
    Err(CommandFailure::new(
        ErrorCode::InternalError,
        "imported telemetry event has no skill identity",
    ))
}

pub(super) fn plan_json(
    plan: &ScanPlan,
    dry_run: bool,
    cursor_advanced: bool,
) -> std::result::Result<Value, CommandFailure> {
    let by_agent = plan
        .agents
        .iter()
        .map(|agent| {
            (
                agent.as_str().to_string(),
                serde_json::to_value(plan.stats.get(agent).cloned().unwrap_or_default())
                    .expect("agent stats serialize"),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let sum = |field: fn(&AgentStats) -> usize, label: &str| {
        checked_map_sum(plan.stats.values().map(field), label)
    };
    let by_skill = summary_json(&plan.matched);
    let unmatched = summary_json(&plan.unmatched);
    Ok(json!({
        "agents": plan.agents.iter().map(|agent| agent.as_str()).collect::<Vec<_>>(),
        "by_agent": by_agent,
        "by_skill": by_skill,
        "since": plan.since.map(|value| value.to_rfc3339()),
        "dry_run": dry_run,
        "scanned_files": sum(|stats| stats.scanned_files, "scanned_files")?,
        "scanned_events": sum(|stats| stats.scanned_events, "scanned_events")?,
        "ingested": sum(|stats| stats.ingested, "ingested")?,
        "duplicates_skipped": sum(|stats| stats.duplicates_skipped, "duplicates_skipped")?,
        "window_skipped": sum(|stats| stats.window_skipped, "window_skipped")?,
        "malformed": sum(|stats| stats.malformed, "malformed")?,
        "pending_partial": sum(|stats| stats.pending_partial, "pending_partial")?,
        "sources_reset": {"count": checked_map_sum(plan.reset_reasons.values().copied(), "sources_reset")?, "reasons": plan.reset_reasons},
        "rejected": {"count": checked_map_sum(plan.rejected_reasons.values().copied(), "rejected")?, "reasons": plan.rejected_reasons},
        "unmatched": unmatched,
        "cursor_advanced": cursor_advanced,
    }))
}

fn summary_json(counts: &SkillSummary) -> Vec<Value> {
    counts
        .iter()
        .map(|((name, agent), count)| json!({"name": name, "agent": agent, "count": count}))
        .collect()
}
