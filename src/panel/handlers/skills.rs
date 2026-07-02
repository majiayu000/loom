use axum::{Json, extract::Path as AxumPath, extract::State, http::StatusCode};
use serde_json::json;

use crate::cli::{SkillInspectArgs, SkillOnlyArgs};
use crate::commands::{App, build_skill_read_model};
use crate::envelope::Envelope;
use crate::types::ErrorCode;

use super::super::PanelState;
use super::common::panel_command_envelope;

pub(in crate::panel) async fn v1_skills(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match build_skill_read_model(&state.ctx) {
        Ok(model) => (
            StatusCode::OK,
            Json(json!(Envelope::ok(
                "registry.skills",
                uuid::Uuid::new_v4().to_string(),
                json!({
                    "state_model": "union",
                    "registry_available": model.registry_available,
                    "count": model.skills.len(),
                    "skills": model.skills,
                }),
                crate::envelope::Meta {
                    warnings: model.warnings,
                    ..crate::envelope::Meta::default()
                }
            ))),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!(Envelope::err(
                "registry.skills",
                uuid::Uuid::new_v4().to_string(),
                ErrorCode::InternalError,
                err.to_string(),
                serde_json::Value::Object(Default::default())
            ))),
        ),
    }
}

pub(in crate::panel) async fn v1_skill_diagnose(
    AxumPath(skill_name): AxumPath<String>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope(
        "skill.diagnose",
        app.cmd_skill_diagnose(&SkillOnlyArgs { skill: skill_name }),
    )
}

pub(in crate::panel) async fn v1_skill_inspect(
    AxumPath(skill_name): AxumPath<String>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope(
        "skill.inspect",
        app.cmd_skill_inspect(&SkillInspectArgs {
            skill: skill_name,
            agent: None,
            workspace: None,
            profile: None,
            include_telemetry: false,
        }),
    )
}

pub(in crate::panel) async fn v1_skill_trash(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    panel_command_envelope("skill.trash.list", app.cmd_skill_trash_list())
}
