use std::path::Path;

use crate::state::AppContext;

use super::super::CommandFailure;
use super::model::{RecommendationFeedback, TelemetryEventType};
use super::store::{read_config, read_event_log, workspace_hash_for_path};

#[derive(Default)]
pub(crate) struct SkillTelemetryEvidence {
    pub(crate) enabled: bool,
    pub(crate) events: usize,
    pub(crate) invocations: u64,
    pub(crate) errors: u64,
    pub(crate) feedback_accepted: u64,
    pub(crate) feedback_rejected: u64,
    pub(crate) feedback_ignored: u64,
}

pub(crate) fn skill_recommendation_telemetry(
    ctx: &AppContext,
    skill: &str,
    agent: Option<&str>,
    workspace: Option<&Path>,
) -> std::result::Result<SkillTelemetryEvidence, CommandFailure> {
    let Some(config) = read_config(ctx)? else {
        return Ok(SkillTelemetryEvidence::default());
    };
    if !config.enabled {
        return Ok(SkillTelemetryEvidence::default());
    }
    let workspace_hash = workspace.map(workspace_hash_for_path);
    let log = read_event_log(ctx)?;
    let mut evidence = SkillTelemetryEvidence {
        enabled: true,
        ..SkillTelemetryEvidence::default()
    };
    for entry in log.events {
        let event = entry.event;
        if event.skill_id.as_deref() != Some(skill) {
            continue;
        }
        if agent.is_some_and(|agent| event.agent.as_deref() != Some(agent)) {
            continue;
        }
        if workspace_hash
            .as_deref()
            .is_some_and(|workspace| event.workspace_hash.as_deref() != Some(workspace))
        {
            continue;
        }
        evidence.events += 1;
        match event.event_type {
            TelemetryEventType::SkillInvocation => evidence.invocations += 1,
            TelemetryEventType::SkillError => evidence.errors += 1,
            TelemetryEventType::RecommendationFeedback => match event.metrics.feedback {
                Some(RecommendationFeedback::Accepted) => evidence.feedback_accepted += 1,
                Some(RecommendationFeedback::Rejected) => evidence.feedback_rejected += 1,
                Some(RecommendationFeedback::Ignored) => evidence.feedback_ignored += 1,
                None => {}
            },
            _ => {}
        }
    }
    Ok(evidence)
}
