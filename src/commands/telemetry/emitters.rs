use std::path::Path;

use serde_json::{Value, json};

use super::super::CommandFailure;
use super::current_workspace;
use super::model::{
    RecommendationFeedback, TelemetryEventDraft, TelemetryEventType, TelemetryMetrics,
};
use super::store::append_event_if_enabled;
use crate::state::AppContext;

pub(crate) struct TelemetryRecordResult {
    pub(crate) event_type: &'static str,
    pub(crate) recorded: bool,
    pub(crate) event_id: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) reason: Option<&'static str>,
    pub(crate) failure_category: Option<String>,
}

pub(crate) struct SkillInvocationTelemetry<'a> {
    pub(crate) agent: Option<&'a str>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) tokens_in: Option<u64>,
    pub(crate) tokens_out: Option<u64>,
    pub(crate) commands: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
}

pub(crate) struct SkillErrorTelemetry<'a> {
    pub(crate) agent: Option<&'a str>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) tokens_in: Option<u64>,
    pub(crate) tokens_out: Option<u64>,
    pub(crate) commands: Option<u64>,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) failure_category: &'a str,
}

pub(crate) struct RecommendationFeedbackTelemetry<'a> {
    pub(crate) feedback: RecommendationFeedback,
    pub(crate) agent: Option<&'a str>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) task: Option<&'a str>,
}

pub(crate) fn record_skill_invocation_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: SkillInvocationTelemetry<'_>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::SkillInvocation);
    populate_common_draft(
        &mut draft,
        skill,
        input.agent,
        input.workspace,
        input.session_id,
        None,
    )?;
    draft.metrics = TelemetryMetrics {
        tokens_in: input.tokens_in,
        tokens_out: input.tokens_out,
        commands: input.commands,
        duration_ms: input.duration_ms,
        success: Some(true),
        ..TelemetryMetrics::default()
    };
    append_record_result(ctx, draft, None)
}

pub(crate) fn record_skill_error_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: SkillErrorTelemetry<'_>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::SkillError);
    populate_common_draft(
        &mut draft,
        skill,
        input.agent,
        input.workspace,
        input.session_id,
        None,
    )?;
    let failure_category = input.failure_category.to_string();
    draft.metrics = TelemetryMetrics {
        tokens_in: input.tokens_in,
        tokens_out: input.tokens_out,
        commands: input.commands,
        duration_ms: input.duration_ms,
        success: Some(false),
        failure_category: Some(failure_category.clone()),
        ..TelemetryMetrics::default()
    };
    append_record_result(ctx, draft, Some(failure_category))
}

pub(crate) fn record_recommendation_feedback_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: RecommendationFeedbackTelemetry<'_>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::RecommendationFeedback);
    populate_common_draft(
        &mut draft,
        skill,
        input.agent,
        input.workspace,
        input.session_id,
        input.task,
    )?;
    draft.metrics.feedback = Some(input.feedback);
    append_record_result(ctx, draft, None)
}

fn populate_common_draft(
    draft: &mut TelemetryEventDraft,
    skill: &str,
    agent: Option<&str>,
    workspace: Option<&Path>,
    session_id: Option<&str>,
    task: Option<&str>,
) -> std::result::Result<(), CommandFailure> {
    draft.skill_id = Some(skill.to_string());
    draft.agent = agent.map(str::to_string);
    draft.workspace = Some(
        workspace
            .map(Path::to_path_buf)
            .unwrap_or(current_workspace()?),
    );
    draft.session_id = session_id.map(str::to_string);
    draft.task = task.map(str::to_string);
    Ok(())
}

fn append_record_result(
    ctx: &AppContext,
    draft: TelemetryEventDraft,
    failure_category: Option<String>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let event_type = draft.event_type.as_str();
    let event = append_event_if_enabled(ctx, draft)?;
    Ok(match event {
        Some(event) => TelemetryRecordResult {
            event_type,
            recorded: true,
            event_id: Some(event.event_id),
            enabled: true,
            reason: None,
            failure_category,
        },
        None => TelemetryRecordResult {
            event_type,
            recorded: false,
            event_id: None,
            enabled: false,
            reason: Some("telemetry_disabled"),
            failure_category,
        },
    })
}

pub(crate) fn instrumentation_json() -> Value {
    let entries = [
        ("skill.activation", &["skill.activate"][..]),
        ("skill.deactivation", &["skill.deactivate"][..]),
        ("skill.invocation", &["skill.used"][..]),
        (
            "skill.eval",
            &[
                "skill.eval",
                "skill.eval run",
                "skill.eval trigger",
                "skill.eval compare",
            ][..],
        ),
        ("skill.safety", &["skill.scan"][..]),
        ("skill.error", &["skill.used --error"][..]),
        ("recommendation.feedback", &["skill.feedback"][..]),
        ("telemetry.sync", &[][..]),
    ];
    let mut out = serde_json::Map::new();
    for (event_type, emitters) in entries {
        out.insert(
            event_type.to_string(),
            json!({
                "status": if emitters.is_empty() { "not_instrumented" } else { "instrumented" },
                "emitters": emitters,
            }),
        );
    }
    Value::Object(out)
}
