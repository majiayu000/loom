use std::path::PathBuf;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::cli::TelemetryReportArgs;
use crate::commands::App;

use super::super::PanelState;
use super::common::panel_command_envelope;

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
    let app = App {
        ctx: state.ctx.as_ref().clone(),
    };
    let args = TelemetryReportArgs::from(query);
    panel_command_envelope("telemetry.report", app.cmd_telemetry_report(&args))
}
