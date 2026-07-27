use axum::{Json, extract::Path as AxumPath, extract::State, http::StatusCode};
use serde_json::json;

use crate::cli::{
    Command, SkillCommand, SkillDiagnoseArgs, SkillDiagnoseCheck, SkillInspectArgs,
    SkillTrashCommand,
};
use crate::commands::build_skill_read_model;
use crate::envelope::Envelope;
use crate::types::ErrorCode;

use super::super::PanelState;
use super::super::auth::run_panel_command;

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
    run_panel_command(
        &state,
        "skill.diagnose",
        StatusCode::OK,
        Command::Skill {
            command: SkillCommand::Diagnose(SkillDiagnoseArgs {
                skill: skill_name,
                agent: None,
                check: SkillDiagnoseCheck::All,
            }),
        },
    )
}

pub(in crate::panel) async fn v1_skill_inspect(
    AxumPath(skill_name): AxumPath<String>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "skill.inspect",
        StatusCode::OK,
        Command::Skill {
            command: SkillCommand::Inspect(SkillInspectArgs {
                skill: skill_name,
                agent: None,
                workspace: None,
                profile: None,
                include_telemetry: false,
                brief: false,
            }),
        },
    )
}

pub(in crate::panel) async fn v1_skill_trash(
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    run_panel_command(
        &state,
        "skill.trash.list",
        StatusCode::OK,
        Command::Skill {
            command: SkillCommand::Trash {
                command: SkillTrashCommand::List,
            },
        },
    )
}
