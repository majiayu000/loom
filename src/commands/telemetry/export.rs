use crate::cli::TelemetryExportFormat;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::store::TelemetryLogEntry;

pub(super) fn export_jsonl(
    entries: &[TelemetryLogEntry],
) -> std::result::Result<String, CommandFailure> {
    let mut body = String::new();
    for entry in entries {
        let raw = serde_json::to_string(&entry.event).map_err(|err| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("failed to encode telemetry event for export: {err}"),
            )
        })?;
        body.push_str(&raw);
        body.push('\n');
    }
    Ok(body)
}

pub(super) fn export_csv(entries: &[TelemetryLogEntry]) -> String {
    let mut body = String::from(
        "schema_version,event_id,event_type,skill_id,observed_skill_name,skillset_id,agent,workspace_hash,session_id_hash,task_hash,timestamp,tokens_in,tokens_out,commands,duration_ms,success,baseline_delta,feedback,safety_findings,dependency_findings,failure_category,raw_prompt_stored,raw_code_stored,redacted\n",
    );
    for entry in entries {
        let event = &entry.event;
        let fields = [
            event.schema_version.to_string(),
            event.event_id.clone(),
            event.event_type.as_str().to_string(),
            event.skill_id.clone().unwrap_or_default(),
            event.observed_skill_name.clone().unwrap_or_default(),
            event.skillset_id.clone().unwrap_or_default(),
            event.agent.clone().unwrap_or_default(),
            event.workspace_hash.clone().unwrap_or_default(),
            event.session_id_hash.clone().unwrap_or_default(),
            event.task_hash.clone().unwrap_or_default(),
            event.timestamp.to_rfc3339(),
            event
                .metrics
                .tokens_in
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .tokens_out
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .commands
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .duration_ms
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .success
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .baseline_delta
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .feedback
                .map(|value| value.as_str().to_string())
                .unwrap_or_default(),
            event
                .metrics
                .safety_findings
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event
                .metrics
                .dependency_findings
                .map(|value| value.to_string())
                .unwrap_or_default(),
            event.metrics.failure_category.clone().unwrap_or_default(),
            event.privacy.raw_prompt_stored.to_string(),
            event.privacy.raw_code_stored.to_string(),
            event.privacy.redacted.to_string(),
        ];
        body.push_str(
            &fields
                .iter()
                .map(|field| csv_escape(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        body.push('\n');
    }
    body
}

pub(super) fn export_format_label(format: TelemetryExportFormat) -> &'static str {
    match format {
        TelemetryExportFormat::Jsonl => "jsonl",
        TelemetryExportFormat::Csv => "csv",
    }
}

fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}
