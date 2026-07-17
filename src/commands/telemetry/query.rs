use chrono::{DateTime, Utc};

use crate::state::AppContext;

use super::super::CommandFailure;
use super::model::{TelemetryConfig, TelemetryEvent, TelemetryEventType};
use super::store::{MalformedTelemetryLine, read_config, read_event_log};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SkillRef {
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
pub(super) enum AgentRef {
    Known(String),
    Unknown,
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedTelemetryRow {
    pub(super) event: TelemetryEvent,
    pub(super) skill_ref: SkillRef,
    pub(super) agent_ref: AgentRef,
}

pub(super) struct NormalizedTelemetryDataset {
    pub(super) telemetry_enabled: bool,
    pub(super) persisted_event_count: usize,
    pub(super) malformed_event_count: usize,
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
}

pub(super) fn load_dataset(
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
            }
        })
        .collect::<Vec<_>>();
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
        })
        .collect()
}

#[allow(dead_code)]
pub(super) fn usage_rows(
    dataset: &NormalizedTelemetryDataset,
) -> impl Iterator<Item = &NormalizedTelemetryRow> {
    dataset.rows.iter().filter(|row| {
        matches!(
            row.event.event_type,
            TelemetryEventType::SkillInvocation | TelemetryEventType::SkillError
        )
    })
}
