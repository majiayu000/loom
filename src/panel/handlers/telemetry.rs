use std::path::PathBuf;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::cli::{Command, TelemetryCommand, TelemetryReportArgs};

use super::super::PanelState;
use super::super::auth::run_panel_command;

#[derive(Debug, Default, Deserialize)]
pub(in crate::panel) struct TelemetryReportQuery {
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    skillset: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    workspace: Option<PathBuf>,
    #[serde(default)]
    since: Option<String>,
}

impl From<TelemetryReportQuery> for TelemetryReportArgs {
    fn from(query: TelemetryReportQuery) -> Self {
        Self {
            skill: query.skill,
            skillset: query.skillset,
            agent: query.agent,
            workspace: query.workspace,
            since: query.since,
        }
    }
}

pub(in crate::panel) async fn v1_telemetry_report(
    Query(query): Query<TelemetryReportQuery>,
    State(state): State<PanelState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let args = TelemetryReportArgs::from(query);
    run_panel_command(
        &state,
        "telemetry.report",
        StatusCode::OK,
        Command::Telemetry {
            command: TelemetryCommand::Report(args),
        },
    )
}
