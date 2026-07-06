use serde_json::{Value, json};

use crate::cli::{SkillFeedbackArgs, SkillUsedArgs};
use crate::envelope::Meta;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_registry_state, validate_skill_name};
use super::telemetry::{
    RecommendationFeedback, RecommendationFeedbackTelemetry, SkillErrorTelemetry,
    SkillInvocationTelemetry, TelemetryRecordResult, failure_category_allowed,
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

fn skill_usage_response(
    skill: &str,
    result: TelemetryRecordResult,
    feedback: Option<RecommendationFeedback>,
) -> Value {
    let mut payload = json!({
        "skill": skill,
        "event_type": result.event_type,
        "recorded": result.recorded,
        "event_id": result.event_id,
        "reason": result.reason,
        "telemetry": {
            "enabled": result.enabled,
            "mode": "local-only",
        },
    });
    if let Some(feedback) = feedback {
        payload["feedback"] = json!(feedback.as_str());
    }
    if let Some(failure_category) = result.failure_category {
        payload["failure_category"] = json!(failure_category);
    }
    payload
}

fn validate_failure_category(raw: &str) -> std::result::Result<(), CommandFailure> {
    if !failure_category_allowed(raw) {
        return Err(map_arg(anyhow::anyhow!("unsupported failure-category")));
    }
    Ok(())
}

fn validate_feedback(raw: &str) -> std::result::Result<RecommendationFeedback, CommandFailure> {
    match raw {
        "accepted" => Ok(RecommendationFeedback::Accepted),
        "rejected" => Ok(RecommendationFeedback::Rejected),
        "ignored" => Ok(RecommendationFeedback::Ignored),
        _ => Err(map_arg(anyhow::anyhow!("unsupported feedback"))),
    }
}

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
