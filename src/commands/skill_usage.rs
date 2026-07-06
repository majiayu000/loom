use serde_json::{Map, Value, json};

use crate::cli::{SkillFeedbackArgs, SkillUsedArgs};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_registry_state, validate_skill_name};
use super::telemetry::{
    RecommendationFeedbackTelemetry, SkillErrorTelemetry, SkillInvocationTelemetry,
    TelemetryRecordResult, failure_category_allowed, feedback_allowed,
    record_recommendation_feedback_telemetry, record_skill_error_telemetry,
    record_skill_invocation_telemetry,
};
use super::{App, CommandFailure, build_skill_read_model};

impl App {
    pub fn cmd_skill_used(
        &self,
        args: &SkillUsedArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_skill_in_read_model(&self.ctx, &args.skill)?;
        if args.error && args.failure_category.is_none() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--failure-category is required with --error",
            ));
        }
        if !args.error && args.failure_category.is_some() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--failure-category requires --error",
            ));
        }

        let result = if args.error {
            let failure_category = args.failure_category.as_deref().unwrap_or_default();
            validate_failure_category(failure_category)?;
            record_skill_error_telemetry(
                &self.ctx,
                &args.skill,
                SkillErrorTelemetry {
                    agent: args.agent.as_deref(),
                    workspace: args.workspace.as_deref(),
                    session_id: args.session_id.as_deref(),
                    tokens_in: args.tokens_in,
                    tokens_out: args.tokens_out,
                    commands: args.commands,
                    duration_ms: args.duration_ms,
                    failure_category,
                },
            )?
        } else {
            record_skill_invocation_telemetry(
                &self.ctx,
                &args.skill,
                SkillInvocationTelemetry {
                    agent: args.agent.as_deref(),
                    workspace: args.workspace.as_deref(),
                    session_id: args.session_id.as_deref(),
                    tokens_in: args.tokens_in,
                    tokens_out: args.tokens_out,
                    commands: args.commands,
                    duration_ms: args.duration_ms,
                },
            )?
        };

        Ok((
            skill_usage_response(&args.skill, result, None),
            Meta::default(),
        ))
    }

    pub fn cmd_skill_feedback(
        &self,
        args: &SkillFeedbackArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        ensure_skill_in_read_model(&self.ctx, &args.skill)?;
        let feedback = validate_feedback(args.feedback.as_str())?;
        let result = record_recommendation_feedback_telemetry(
            &self.ctx,
            &args.skill,
            RecommendationFeedbackTelemetry {
                feedback,
                agent: args.agent.as_deref(),
                workspace: args.workspace.as_deref(),
                session_id: args.session_id.as_deref(),
                task: args.task.as_deref(),
            },
        )?;

        Ok((
            skill_usage_response(&args.skill, result, Some(feedback)),
            Meta::default(),
        ))
    }
}

#[inline(never)]
fn skill_usage_response(
    skill: &str,
    result: TelemetryRecordResult,
    feedback: Option<&str>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("skill".to_string(), json!(skill));
    payload.insert("event_type".to_string(), json!(result.event_type));
    payload.insert("recorded".to_string(), json!(result.recorded));
    payload.insert("event_id".to_string(), json!(result.event_id));
    payload.insert("reason".to_string(), json!(result.reason));
    payload.insert(
        "telemetry".to_string(),
        json!({
            "enabled": result.enabled,
            "mode": result.mode,
        }),
    );
    if let Some(feedback) = feedback {
        payload.insert("feedback".to_string(), json!(feedback));
    }
    if let Some(failure_category) = result.failure_category {
        payload.insert("failure_category".to_string(), json!(failure_category));
    }
    Value::Object(payload)
}

#[inline(never)]
fn validate_failure_category(raw: &str) -> std::result::Result<(), CommandFailure> {
    if !failure_category_allowed(raw) {
        return Err(map_arg(anyhow::anyhow!("unsupported failure-category")));
    }
    Ok(())
}

#[inline(never)]
fn validate_feedback(raw: &str) -> std::result::Result<&str, CommandFailure> {
    if feedback_allowed(raw) {
        Ok(raw)
    } else {
        Err(map_arg(anyhow::anyhow!("unsupported feedback")))
    }
}

#[inline(never)]
fn ensure_skill_in_read_model(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(skill).map_err(map_arg)?;
    let model = build_skill_read_model(ctx).map_err(map_registry_state)?;
    if model
        .skills
        .iter()
        .any(|row| row["skill_id"].as_str() == Some(skill))
    {
        Ok(())
    } else {
        Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skill '{}' not found", skill),
        ))
    }
}
