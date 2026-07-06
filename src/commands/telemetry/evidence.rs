use std::path::Path;

use chrono::{DateTime, Duration, Utc};

use crate::state::AppContext;

use super::super::CommandFailure;
use super::model::{RecommendationFeedback, TelemetryConfig, TelemetryEvent, TelemetryEventType};
use super::store::{read_config, read_event_log, task_hash_for_text, workspace_hash_for_path};

#[derive(Clone, Default)]
pub(crate) struct SkillTelemetryEvidence {
    pub(crate) enabled: bool,
    pub(crate) events: usize,
    pub(crate) invocations: u64,
    pub(crate) errors: u64,
    pub(crate) feedback_accepted: u64,
    pub(crate) feedback_rejected: u64,
    pub(crate) feedback_ignored: u64,
}

#[derive(Default)]
pub(crate) struct SkillTelemetryEvidenceCache {
    state: Option<SkillTelemetryEvidenceState>,
}

struct SkillTelemetryEvidenceState {
    enabled: bool,
    cutoff: DateTime<Utc>,
    events: Vec<TelemetryEvent>,
}

impl SkillTelemetryEvidenceCache {
    pub(crate) fn evidence_for(
        &mut self,
        ctx: &AppContext,
        skill: &str,
        agent: Option<&str>,
        workspace: Option<&Path>,
        task: Option<&str>,
    ) -> std::result::Result<SkillTelemetryEvidence, CommandFailure> {
        if self.state.is_none() {
            self.state = Some(load_state(ctx)?);
        }
        let Some(state) = self.state.as_ref() else {
            return Ok(SkillTelemetryEvidence::default());
        };
        if !state.enabled {
            return Ok(SkillTelemetryEvidence::default());
        }
        let workspace_hash = workspace.map(workspace_hash_for_path);
        let task_hash = task.map(task_hash_for_text);
        let mut evidence = SkillTelemetryEvidence {
            enabled: true,
            ..SkillTelemetryEvidence::default()
        };
        for event in &state.events {
            if event.skill_id.as_deref() != Some(skill) {
                continue;
            }
            if agent.is_some_and(|agent| {
                event
                    .agent
                    .as_deref()
                    .is_some_and(|event_agent| event_agent != agent)
            }) {
                continue;
            }
            if workspace_hash
                .as_deref()
                .is_some_and(|workspace| event.workspace_hash.as_deref() != Some(workspace))
            {
                continue;
            }
            if event.timestamp < state.cutoff {
                continue;
            }
            evidence.events += 1;
            match event.event_type {
                TelemetryEventType::SkillInvocation => evidence.invocations += 1,
                TelemetryEventType::SkillError => evidence.errors += 1,
                TelemetryEventType::RecommendationFeedback => match event.metrics.feedback {
                    Some(feedback) if feedback_matches_task(event, task_hash.as_deref()) => {
                        match feedback {
                            RecommendationFeedback::Accepted => evidence.feedback_accepted += 1,
                            RecommendationFeedback::Rejected => evidence.feedback_rejected += 1,
                            RecommendationFeedback::Ignored => evidence.feedback_ignored += 1,
                        }
                    }
                    Some(_) | None => {}
                },
                _ => {}
            }
        }
        Ok(evidence)
    }
}

fn feedback_matches_task(event: &TelemetryEvent, task_hash: Option<&str>) -> bool {
    match task_hash {
        Some(task_hash) => match event.task_hash.as_deref() {
            Some(event_task_hash) => event_task_hash == task_hash,
            None => true,
        },
        None => true,
    }
}

fn load_state(
    ctx: &AppContext,
) -> std::result::Result<SkillTelemetryEvidenceState, CommandFailure> {
    let config = match read_config(ctx) {
        Ok(Some(config)) => config,
        Ok(None) => return Ok(disabled_state()),
        Err(err) => return Err(err),
    };
    if !config.enabled {
        return Ok(disabled_state());
    }
    let log = read_event_log(ctx)?;
    Ok(SkillTelemetryEvidenceState {
        enabled: true,
        cutoff: cutoff_for_config(&config),
        events: log.events.into_iter().map(|entry| entry.event).collect(),
    })
}

fn disabled_state() -> SkillTelemetryEvidenceState {
    SkillTelemetryEvidenceState {
        enabled: false,
        cutoff: Utc::now(),
        events: Vec::new(),
    }
}

fn cutoff_for_config(config: &TelemetryConfig) -> DateTime<Utc> {
    Utc::now() - Duration::days(i64::from(config.retention_days))
}
