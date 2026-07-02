use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum TelemetryCommand {
    #[command(about = "Show local telemetry enablement, storage, retention, and privacy state")]
    Status,
    #[command(about = "Enable local-only telemetry")]
    Enable(TelemetryEnableArgs),
    #[command(about = "Disable local telemetry writes")]
    Disable,
    #[command(about = "Aggregate local telemetry events into a privacy-preserving report")]
    Report(TelemetryReportArgs),
    #[command(about = "Export redacted telemetry events to JSONL or CSV")]
    Export(TelemetryExportArgs),
    #[command(about = "Preview or confirm deletion of selected telemetry events")]
    Purge(TelemetryPurgeArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TelemetryEnableArgs {
    /// Store telemetry only in the local registry.
    #[arg(long)]
    pub local_only: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TelemetryReportArgs {
    /// Limit the report to one skill.
    #[arg(long)]
    pub skill: Option<String>,

    /// Limit the report to one skillset when events include skillset evidence.
    #[arg(long)]
    pub skillset: Option<String>,

    /// Limit the report to one agent id.
    #[arg(long)]
    pub agent: Option<String>,

    /// Limit the report to one workspace path after hashing.
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Include events at or after this date or RFC3339 timestamp.
    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TelemetryExportArgs {
    /// Export format.
    #[arg(long, value_enum)]
    pub format: TelemetryExportFormat,

    /// Output path outside registry state.
    #[arg(long)]
    pub output: PathBuf,

    /// Request redacted output. Exports are redacted by default.
    #[arg(long)]
    pub redacted: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryExportFormat {
    Jsonl,
    Csv,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct TelemetryPurgeArgs {
    /// Delete events before this date or RFC3339 timestamp.
    #[arg(long)]
    pub before: Option<String>,

    /// Preview the purge without mutating telemetry state.
    #[arg(long, conflicts_with = "confirm")]
    pub dry_run: bool,

    /// Confirmation token returned by a matching dry-run.
    #[arg(long, conflicts_with = "dry_run")]
    pub confirm: Option<String>,
}
