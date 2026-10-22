use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};

use crate::cli::TelemetryOrigin;
use crate::state::AppContext;

use super::super::CommandFailure;
use super::model::{SessionOrigin, TelemetryConfig, TelemetryEvent, TelemetryEventType};
use super::store::{MalformedTelemetryLine, read_config, read_event_log};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SkillRef {
    Registered(String),
    Observed(String),
    Unattributed,
}

impl SkillRef {
    pub(super) fn label(&self) -> Option<&str> {
        match self {
            Self::Registered(value) | Self::Observed(value) => Some(value),
            Self::Unattributed => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AgentRef {
    Known(String),
    Unknown,
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedTelemetryRow {
    pub(super) event: TelemetryEvent,
    pub(super) skill_ref: SkillRef,
    pub(super) agent_ref: AgentRef,
    /// The invocation came from one agent turn that read many different skills: an
    /// inventory or audit pass, not use of each skill. Excluded from usage counts.
    pub(super) audit_sweep: bool,
}

/// One turn that loads at least this many distinct skills is treated as a sweep.
pub(crate) const AUDIT_SWEEP_MIN_SKILLS: usize = 8;

pub(crate) struct NormalizedTelemetryDataset {
    pub(crate) telemetry_enabled: bool,
    pub(crate) persisted_event_count: usize,
    pub(crate) malformed_event_count: usize,
    pub(super) config: Option<TelemetryConfig>,
    pub(super) rows: Vec<NormalizedTelemetryRow>,
    pub(super) malformed: Vec<MalformedTelemetryLine>,
}

#[derive(Default)]
pub(super) struct TelemetryFilters {
    pub(super) skill: Option<String>,
    pub(super) skillset: Option<String>,
    pub(super) agent: Option<String>,
    pub(super) workspace_hash: Option<String>,
    pub(super) since: Option<DateTime<Utc>>,
    pub(super) origin: Option<TelemetryOrigin>,
}

pub(crate) fn load_dataset(
    ctx: &AppContext,
) -> std::result::Result<NormalizedTelemetryDataset, CommandFailure> {
    let config = read_config(ctx)?;
    let log = read_event_log(ctx)?;
    let rows = log
        .events
        .into_iter()
        .map(|entry| {
            let skill_ref = match (
                entry.event.skill_id.as_ref(),
                entry.event.observed_skill_name.as_ref(),
            ) {
                (Some(skill), None) => SkillRef::Registered(skill.clone()),
                (None, Some(skill)) => SkillRef::Observed(skill.clone()),
                _ => SkillRef::Unattributed,
            };
            let agent_ref = entry
                .event
                .agent
                .as_ref()
                .map_or(AgentRef::Unknown, |agent| AgentRef::Known(agent.clone()));
            NormalizedTelemetryRow {
                event: entry.event,
                skill_ref,
                agent_ref,
                audit_sweep: false,
            }
        })
        .collect::<Vec<_>>();
    let rows = mark_audit_sweeps(rows);
    Ok(NormalizedTelemetryDataset {
        telemetry_enabled: config.as_ref().is_some_and(|config| config.enabled),
        persisted_event_count: rows.len(),
        malformed_event_count: log.malformed.len(),
        config,
        rows,
        malformed: log.malformed,
    })
}

pub(super) fn filtered_rows<'a>(
    dataset: &'a NormalizedTelemetryDataset,
    filters: &TelemetryFilters,
) -> Vec<&'a NormalizedTelemetryRow> {
    dataset
        .rows
        .iter()
        .filter(|row| {
            filters
                .skill
                .as_deref()
                .is_none_or(|skill| row.skill_ref.label() == Some(skill))
                && filters
                    .skillset
                    .as_deref()
                    .is_none_or(|skillset| row.event.skillset_id.as_deref() == Some(skillset))
                && filters.agent.as_deref().is_none_or(
                    |agent| matches!(&row.agent_ref, AgentRef::Known(value) if value == agent),
                )
                && filters
                    .workspace_hash
                    .as_deref()
                    .is_none_or(|workspace| row.event.workspace_hash.as_deref() == Some(workspace))
                && filters
                    .since
                    .is_none_or(|since| row.event.timestamp >= since)
                && filters
                    .origin
                    .is_none_or(|origin| origin_matches(origin, row.event.origin))
        })
        .collect()
}

pub(crate) fn origin_matches(filter: TelemetryOrigin, origin: Option<SessionOrigin>) -> bool {
    match filter {
        TelemetryOrigin::Interactive => origin == Some(SessionOrigin::Interactive),
        TelemetryOrigin::Automated => origin == Some(SessionOrigin::Automated),
        TelemetryOrigin::Unknown => origin.is_none(),
    }
}

fn mark_audit_sweeps(mut rows: Vec<NormalizedTelemetryRow>) -> Vec<NormalizedTelemetryRow> {
    let mut skills_per_turn = BTreeMap::<(&str, &str, &str), BTreeSet<&str>>::new();
    for row in &rows {
        if let Some(key) = sweep_key(row)
            && let Some(skill) = row.skill_ref.label()
        {
            skills_per_turn.entry(key).or_default().insert(skill);
        }
    }
    let sweeps = skills_per_turn
        .into_iter()
        .filter(|(_, skills)| skills.len() >= AUDIT_SWEEP_MIN_SKILLS)
        .map(|(key, _)| (key.0.to_string(), key.1.to_string(), key.2.to_string()))
        .collect::<BTreeSet<_>>();
    if sweeps.is_empty() {
        return rows;
    }
    for row in &mut rows {
        row.audit_sweep = sweep_key(row).is_some_and(|(agent, session, turn)| {
            sweeps.contains(&(agent.to_string(), session.to_string(), turn.to_string()))
        });
    }
    rows
}

fn sweep_key(row: &NormalizedTelemetryRow) -> Option<(&str, &str, &str)> {
    if row.event.event_type != TelemetryEventType::SkillInvocation {
        return None;
    }
    Some((
        row.event.agent.as_deref().unwrap_or(""),
        row.event.session_id_hash.as_deref()?,
        row.event.turn_hash.as_deref()?,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageKind {
    Invocation,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct UsageRow {
    pub(crate) skill_ref: SkillRef,
    pub(crate) agent_ref: AgentRef,
    pub(crate) kind: UsageKind,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) failure_category: Option<String>,
    pub(crate) origin: Option<SessionOrigin>,
}

pub(crate) fn usage_rows(
    dataset: &NormalizedTelemetryDataset,
) -> impl Iterator<Item = UsageRow> + '_ {
    dataset.rows.iter().filter_map(|row| {
        if row.audit_sweep {
            return None;
        }
        let kind = match row.event.event_type {
            TelemetryEventType::SkillInvocation => UsageKind::Invocation,
            TelemetryEventType::SkillError => UsageKind::Error,
            _ => return None,
        };
        Some(UsageRow {
            skill_ref: row.skill_ref.clone(),
            agent_ref: row.agent_ref.clone(),
            kind,
            timestamp: row.event.timestamp,
            failure_category: row.event.metrics.failure_category.clone(),
            origin: row.event.origin,
        })
    })
}
