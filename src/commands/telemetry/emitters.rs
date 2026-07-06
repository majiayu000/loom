use std::path::Path;

use serde_json::{Value, json};

use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::current_workspace;
use super::model::{
    RecommendationFeedback, TelemetryConfig, TelemetryEventDraft, TelemetryEventType,
    TelemetryMetrics,
};
use super::store::{append_event_if_enabled, read_config};

pub(crate) struct TelemetryRecordResult {
    pub(crate) event_type: &'static str,
    pub(crate) recorded: bool,
    pub(crate) event_id: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) mode: String,
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
    pub(crate) feedback: &'a str,
    pub(crate) agent: Option<&'a str>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) session_id: Option<&'a str>,
}

pub(crate) fn record_skill_invocation_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: SkillInvocationTelemetry<'_>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::SkillInvocation);
    draft.skill_id = Some(skill.to_string());
    draft.agent = input.agent.map(str::to_string);
    draft.workspace = Some(
        input
            .workspace
            .map(Path::to_path_buf)
            .unwrap_or(current_workspace()?),
    );
    draft.session_id = input.session_id.map(str::to_string);
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
    draft.skill_id = Some(skill.to_string());
    draft.agent = input.agent.map(str::to_string);
    draft.workspace = Some(
        input
            .workspace
            .map(Path::to_path_buf)
            .unwrap_or(current_workspace()?),
    );
    draft.session_id = input.session_id.map(str::to_string);
    draft.metrics = TelemetryMetrics {
        tokens_in: input.tokens_in,
        tokens_out: input.tokens_out,
        commands: input.commands,
        duration_ms: input.duration_ms,
        success: Some(false),
        failure_category: Some(input.failure_category.to_string()),
        ..TelemetryMetrics::default()
    };
    append_record_result(ctx, draft, Some(input.failure_category.to_string()))
}

pub(crate) fn record_recommendation_feedback_telemetry(
    ctx: &AppContext,
    skill: &str,
    input: RecommendationFeedbackTelemetry<'_>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let feedback = match input.feedback {
        "accepted" => RecommendationFeedback::Accepted,
        "rejected" => RecommendationFeedback::Rejected,
        "ignored" => RecommendationFeedback::Ignored,
        other => {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("unsupported recommendation feedback '{other}'"),
            ));
        }
    };
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::RecommendationFeedback);
    draft.skill_id = Some(skill.to_string());
    draft.agent = input.agent.map(str::to_string);
    draft.workspace = Some(
        input
            .workspace
            .map(Path::to_path_buf)
            .unwrap_or(current_workspace()?),
    );
    draft.session_id = input.session_id.map(str::to_string);
    draft.metrics.feedback = Some(feedback);
    append_record_result(ctx, draft, None)
}

fn append_record_result(
    ctx: &AppContext,
    draft: TelemetryEventDraft,
    failure_category: Option<String>,
) -> std::result::Result<TelemetryRecordResult, CommandFailure> {
    let event_type = draft.event_type.as_str();
    let config = read_config(ctx)?;
    let effective = config
        .clone()
        .unwrap_or_else(TelemetryConfig::disabled_local);
    let event = append_event_if_enabled(ctx, draft)?;
    Ok(match event {
        Some(event) => TelemetryRecordResult {
            event_type,
            recorded: true,
            event_id: Some(event.event_id),
            enabled: true,
            mode: effective.mode.as_str().to_string(),
            reason: None,
            failure_category,
        },
        None => TelemetryRecordResult {
            event_type,
            recorded: false,
            event_id: None,
            enabled: false,
            mode: effective.mode.as_str().to_string(),
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
