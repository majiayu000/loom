mod emitters;
mod evidence;
mod export;
mod model;
mod store;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::{
    TelemetryCommand, TelemetryEnableArgs, TelemetryExportArgs, TelemetryExportFormat,
    TelemetryPurgeArgs, TelemetryReportArgs,
};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::helpers::{map_io, map_lock, validate_skill_name};
use super::{App, CommandFailure};
pub(crate) use emitters::{
    RecommendationFeedbackTelemetry, SkillErrorTelemetry, SkillInvocationTelemetry,
    TelemetryRecordResult, record_recommendation_feedback_telemetry, record_skill_error_telemetry,
    record_skill_invocation_telemetry,
};
pub(crate) use evidence::SkillTelemetryEvidenceCache;
use export::{export_csv, export_format_label, export_jsonl};
use model::{
    RecommendationFeedback, TelemetryConfig, TelemetryEventDraft, TelemetryEventType,
    TelemetryMetrics,
};
pub(crate) use model::{failure_category_allowed, feedback_allowed};
use store::{
    MalformedTelemetryLine, TelemetryLog, TelemetryLogEntry, config_path, events_path,
    output_path_outside_state, parse_cutoff, purge_token, read_config, read_event_log,
    telemetry_dir, workspace_hash_for_path, write_config,
};

impl App {
    pub fn cmd_telemetry(
        &self,
        command: &TelemetryCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            TelemetryCommand::Status => self.cmd_telemetry_status(),
            TelemetryCommand::Enable(args) => self.cmd_telemetry_enable(args),
            TelemetryCommand::Disable => self.cmd_telemetry_disable(),
            TelemetryCommand::Report(args) => self.cmd_telemetry_report(args),
            TelemetryCommand::Export(args) => self.cmd_telemetry_export(args),
            TelemetryCommand::Purge(args) => self.cmd_telemetry_purge(args),
        }
    }

    fn cmd_telemetry_status(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        let config = read_config(&self.ctx)?;
        let log = read_event_log(&self.ctx)?;
        let effective = config
            .clone()
            .unwrap_or_else(TelemetryConfig::disabled_local);
        Ok((
            json!({
                "schema_version": model::TELEMETRY_SCHEMA_VERSION,
                "configured": config.is_some(),
                "enabled": effective.enabled,
                "mode": effective.mode.as_str(),
                "retention_days": effective.retention_days,
                "storage": storage_json(&self.ctx),
                "privacy": privacy_json(effective.redaction.as_str()),
                "events": {
                    "count": log.events.len(),
                    "malformed_count": log.malformed.len(),
                },
            }),
            Meta {
                warnings: malformed_warnings(&log.malformed),
                ..Meta::default()
            },
        ))
    }

    fn cmd_telemetry_enable(
        &self,
        args: &TelemetryEnableArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        self.ctx.ensure_gitignore_entries().map_err(map_io)?;
        let config = TelemetryConfig::enabled_local();
        write_config(&self.ctx, &config)?;
        Ok((
            json!({
                "enabled": true,
                "mode": config.mode.as_str(),
                "local_only": true,
                "requested_local_only": args.local_only,
                "hosted_configured": false,
                "retention_days": config.retention_days,
                "storage": storage_json(&self.ctx),
                "privacy": privacy_json(config.redaction.as_str()),
            }),
            Meta::default(),
        ))
    }

    fn cmd_telemetry_disable(&self) -> std::result::Result<(Value, Meta), CommandFailure> {
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let config = TelemetryConfig::disabled_local();
        write_config(&self.ctx, &config)?;
        Ok((
            json!({
                "enabled": false,
                "mode": config.mode.as_str(),
                "storage": storage_json(&self.ctx),
                "privacy": privacy_json(config.redaction.as_str()),
            }),
            Meta::default(),
        ))
    }

    pub(crate) fn cmd_telemetry_report(
        &self,
        args: &TelemetryReportArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let filters = filters_from_args(args)?;
        let config = read_config(&self.ctx)?;
        let log = read_event_log(&self.ctx)?;
        let report = build_report(&self.ctx, config.as_ref(), &log, &filters)?;
        Ok((
            report,
            Meta {
                warnings: malformed_warnings(&log.malformed),
                ..Meta::default()
            },
        ))
    }

    fn cmd_telemetry_export(
        &self,
        args: &TelemetryExportArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let output = output_path_outside_state(&self.ctx, &args.output)?;
        let log = read_event_log(&self.ctx)?;
        let body = match args.format {
            TelemetryExportFormat::Jsonl => export_jsonl(&log.events)?,
            TelemetryExportFormat::Csv => export_csv(&log.events),
        };
        write_atomic(&output, &body).map_err(map_io)?;
        Ok((
            json!({
                "format": export_format_label(args.format),
                "output": output.display().to_string(),
                "redacted": true,
                "requested_redacted": args.redacted,
                "events_exported": log.events.len(),
                "malformed_events_skipped": log.malformed.len(),
            }),
            Meta {
                warnings: malformed_warnings(&log.malformed),
                ..Meta::default()
            },
        ))
    }

    fn cmd_telemetry_purge(
        &self,
        args: &TelemetryPurgeArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        if !args.dry_run && args.confirm.is_none() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "telemetry purge requires --dry-run or --confirm <token>",
            ));
        }
        let before = parse_cutoff("--before", args.before.as_deref())?;
        if args.dry_run {
            let log = read_event_log(&self.ctx)?;
            let plan = purge_plan(&log, before);
            return Ok((
                json!({
                    "dry_run": true,
                    "before": before.map(|value| value.to_rfc3339()),
                    "matching_events": plan.matching_events,
                    "matching_bytes": plan.matching_bytes,
                    "malformed_events_preserved": log.malformed.len(),
                    "confirm_token": plan.confirm_token,
                }),
                Meta {
                    warnings: malformed_warnings(&log.malformed),
                    ..Meta::default()
                },
            ));
        }

        let confirm = args.confirm.as_deref().unwrap_or_default();
        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_layout()?;
        let log = read_event_log(&self.ctx)?;
        let plan = purge_plan(&log, before);
        if confirm != plan.confirm_token {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "telemetry purge confirmation token does not match current dry-run plan",
            ));
        }

        let mut remaining = Vec::new();
        for entry in &log.events {
            if !event_before(entry, before) {
                remaining.push(serde_json::to_string(&entry.event).map_err(|err| {
                    CommandFailure::new(
                        ErrorCode::InternalError,
                        format!("failed to encode retained telemetry event: {err}"),
                    )
                })?);
            }
        }
        for line in &log.malformed {
            remaining.push(line.raw.clone());
        }
        let body = if remaining.is_empty() {
            String::new()
        } else {
            remaining.join("\n") + "\n"
        };
        write_atomic(&events_path(&self.ctx), &body).map_err(map_io)?;
        Ok((
            json!({
                "dry_run": false,
                "before": before.map(|value| value.to_rfc3339()),
                "deleted_events": plan.matching_events,
                "deleted_bytes": plan.matching_bytes,
                "malformed_events_preserved": log.malformed.len(),
                "events_path": events_path(&self.ctx).display().to_string(),
            }),
            Meta {
                warnings: malformed_warnings(&log.malformed),
                ..Meta::default()
            },
        ))
    }
}

pub(crate) fn record_skill_activation_telemetry(
    ctx: &AppContext,
    skill: &str,
    agent: &str,
    activated: bool,
    workspace: Option<&Path>,
) -> std::result::Result<(), CommandFailure> {
    let mut draft = TelemetryEventDraft::new(if activated {
        TelemetryEventType::SkillActivation
    } else {
        TelemetryEventType::SkillDeactivation
    });
    draft.skill_id = Some(skill.to_string());
    draft.agent = Some(agent.to_string());
    draft.workspace = Some(
        workspace
            .map(Path::to_path_buf)
            .unwrap_or(current_workspace()?),
    );
    draft.metrics.success = Some(true);
    store::append_event_if_enabled(ctx, draft)?;
    Ok(())
}

pub(crate) fn record_skill_eval_telemetry(
    ctx: &AppContext,
    skill: &str,
    agent: Option<&str>,
    success: bool,
    tokens: Option<u64>,
    commands: Option<u64>,
    baseline_delta: Option<f64>,
) -> std::result::Result<(), CommandFailure> {
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::SkillEval);
    draft.skill_id = Some(skill.to_string());
    draft.agent = agent.map(str::to_string);
    draft.workspace = Some(current_workspace()?);
    draft.metrics = TelemetryMetrics {
        tokens_in: tokens,
        commands,
        success: Some(success),
        baseline_delta,
        ..TelemetryMetrics::default()
    };
    store::append_event_if_enabled(ctx, draft)?;
    Ok(())
}

pub(crate) fn telemetry_warning(action: &str, err: &CommandFailure) -> String {
    format!(
        "telemetry event for {action} was not recorded: {}: {}",
        err.code.as_str(),
        err.message
    )
}

pub(crate) fn record_skill_safety_telemetry(
    ctx: &AppContext,
    skill: &str,
    findings: u64,
    success: bool,
) -> std::result::Result<(), CommandFailure> {
    let mut draft = TelemetryEventDraft::new(TelemetryEventType::SkillSafety);
    draft.skill_id = Some(skill.to_string());
    draft.workspace = Some(current_workspace()?);
    draft.metrics.safety_findings = Some(findings);
    draft.metrics.success = Some(success);
    store::append_event_if_enabled(ctx, draft)?;
    Ok(())
}

pub(crate) fn skill_telemetry_summary(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<Value, CommandFailure> {
    validate_skill_name(skill).map_err(super::helpers::map_arg)?;
    let filters = TelemetryFilters {
        skill: Some(skill.to_string()),
        ..TelemetryFilters::default()
    };
    let config = read_config(ctx)?;
    let log = read_event_log(ctx)?;
    let entries = filtered_events(&log.events, &filters);
    let aggregate = aggregate_entries(&entries);
    Ok(json!({
        "enabled": config.as_ref().is_some_and(|config| config.enabled),
        "configured": config.is_some(),
        "status": if config.as_ref().is_some_and(|config| config.enabled) { "enabled" } else { "disabled" },
        "events": aggregate.events,
        "malformed_events": log.malformed.len(),
        "last_event_at": aggregate.last_event_at.map(|value| value.to_rfc3339()),
        "last_invoked_at": aggregate.last_invoked_at.map(|value| value.to_rfc3339()),
        "last_eval_at": aggregate.last_eval_at.map(|value| value.to_rfc3339()),
        "last_successful_eval_at": aggregate.last_success_eval_at.map(|value| value.to_rfc3339()),
        "last_error_at": aggregate.last_error_at.map(|value| value.to_rfc3339()),
        "usage": usage_json(&aggregate),
        "value": value_json(&aggregate),
        "cost": cost_json(&aggregate),
        "sync": sync_json(),
        "risk": risk_json(&aggregate),
        "recommendation_feedback": feedback_json(&aggregate),
        "instrumentation": emitters::instrumentation_json(),
    }))
}

#[derive(Default)]
struct TelemetryFilters {
    skill: Option<String>,
    skillset: Option<String>,
    agent: Option<String>,
    workspace_hash: Option<String>,
    since: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct Aggregate {
    events: usize,
    activations: u64,
    deactivations: u64,
    invocations: u64,
    errors: u64,
    eval_runs: u64,
    eval_passed: u64,
    eval_failed: u64,
    tokens_in: u64,
    tokens_out: u64,
    commands: u64,
    duration_ms: u64,
    cost_seen: bool,
    baseline_deltas: Vec<f64>,
    safety_events: u64,
    safety_findings: u64,
    dependency_findings: u64,
    feedback_accepted: u64,
    feedback_rejected: u64,
    feedback_ignored: u64,
    last_event_at: Option<DateTime<Utc>>,
    last_invoked_at: Option<DateTime<Utc>>,
    last_eval_at: Option<DateTime<Utc>>,
    last_success_eval_at: Option<DateTime<Utc>>,
    last_error_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct PurgePlan {
    matching_events: usize,
    matching_bytes: usize,
    confirm_token: String,
}

fn filters_from_args(
    args: &TelemetryReportArgs,
) -> std::result::Result<TelemetryFilters, CommandFailure> {
    if let Some(skill) = args.skill.as_deref() {
        validate_skill_name(skill).map_err(super::helpers::map_arg)?;
    }
    if let Some(skillset) = args.skillset.as_deref() {
        validate_skill_name(skillset).map_err(super::helpers::map_arg)?;
    }
    let since = parse_cutoff("--since", args.since.as_deref())?;
    Ok(TelemetryFilters {
        skill: args.skill.clone(),
        skillset: args.skillset.clone(),
        agent: args.agent.clone(),
        workspace_hash: args
            .workspace
            .as_ref()
            .map(|path| workspace_hash_for_path(path)),
        since,
    })
}

fn build_report(
    ctx: &AppContext,
    config: Option<&TelemetryConfig>,
    log: &TelemetryLog,
    filters: &TelemetryFilters,
) -> std::result::Result<Value, CommandFailure> {
    let effective = config
        .cloned()
        .unwrap_or_else(TelemetryConfig::disabled_local);
    let entries = filtered_events(&log.events, filters);
    let aggregate = aggregate_entries(&entries);
    let mut skills: BTreeMap<String, Value> = BTreeMap::new();
    for entry in &entries {
        let skill = entry
            .event
            .skill_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let skill_entries = entries
            .iter()
            .copied()
            .filter(|candidate| candidate.event.skill_id.as_deref() == Some(&skill))
            .collect::<Vec<_>>();
        skills.insert(skill, aggregate_json(&aggregate_entries(&skill_entries)));
    }
    Ok(json!({
        "schema_version": model::TELEMETRY_SCHEMA_VERSION,
        "enabled": effective.enabled,
        "mode": effective.mode.as_str(),
        "retention_days": effective.retention_days,
        "storage": storage_json(ctx),
        "privacy": privacy_json(effective.redaction.as_str()),
        "filters": filters_json(filters),
        "events_total": log.events.len(),
        "matched_events": entries.len(),
        "malformed_events": malformed_json(&log.malformed),
        "instrumentation": emitters::instrumentation_json(),
        "summary": aggregate_json(&aggregate),
        "skills": skills,
        "panel_read_model": {
            "status": "available",
            "deferred_ui": false,
            "route": "/api/v1/telemetry/report"
        }
    }))
}

fn aggregate_entries(entries: &[&TelemetryLogEntry]) -> Aggregate {
    let mut aggregate = Aggregate {
        events: entries.len(),
        ..Aggregate::default()
    };
    for entry in entries {
        let event = &entry.event;
        max_timestamp(&mut aggregate.last_event_at, event.timestamp);
        match event.event_type {
            TelemetryEventType::SkillActivation => aggregate.activations += 1,
            TelemetryEventType::SkillDeactivation => aggregate.deactivations += 1,
            TelemetryEventType::SkillInvocation => {
                aggregate.invocations += 1;
                max_timestamp(&mut aggregate.last_invoked_at, event.timestamp);
            }
            TelemetryEventType::SkillEval => {
                aggregate.eval_runs += 1;
                max_timestamp(&mut aggregate.last_eval_at, event.timestamp);
                match event.metrics.success {
                    Some(true) => {
                        aggregate.eval_passed += 1;
                        max_timestamp(&mut aggregate.last_success_eval_at, event.timestamp);
                    }
                    Some(false) => aggregate.eval_failed += 1,
                    None => {}
                }
                if let Some(delta) = event.metrics.baseline_delta {
                    aggregate.baseline_deltas.push(delta);
                }
            }
            TelemetryEventType::SkillSafety => aggregate.safety_events += 1,
            TelemetryEventType::SkillError => {
                aggregate.errors += 1;
                max_timestamp(&mut aggregate.last_error_at, event.timestamp);
            }
            TelemetryEventType::RecommendationFeedback => match event.metrics.feedback {
                Some(RecommendationFeedback::Accepted) => aggregate.feedback_accepted += 1,
                Some(RecommendationFeedback::Rejected) => aggregate.feedback_rejected += 1,
                Some(RecommendationFeedback::Ignored) => aggregate.feedback_ignored += 1,
                None => {}
            },
        }
        if event.metrics.has_cost() {
            aggregate.cost_seen = true;
        }
        aggregate.tokens_in += event.metrics.tokens_in.unwrap_or(0);
        aggregate.tokens_out += event.metrics.tokens_out.unwrap_or(0);
        aggregate.commands += event.metrics.commands.unwrap_or(0);
        aggregate.duration_ms += event.metrics.duration_ms.unwrap_or(0);
        aggregate.safety_findings += event.metrics.safety_findings.unwrap_or(0);
        aggregate.dependency_findings += event.metrics.dependency_findings.unwrap_or(0);
    }
    aggregate
}

fn aggregate_json(aggregate: &Aggregate) -> Value {
    json!({
        "events": aggregate.events,
        "usage": usage_json(aggregate),
        "value": value_json(aggregate),
        "cost": cost_json(aggregate),
        "sync": sync_json(),
        "drift": drift_json(aggregate),
        "risk": risk_json(aggregate),
        "recommendation_feedback": feedback_json(aggregate),
    })
}

fn usage_json(aggregate: &Aggregate) -> Value {
    json!({
        "activations": aggregate.activations,
        "deactivations": aggregate.deactivations,
        "invocations": aggregate.invocations,
        "errors": aggregate.errors,
        "last_invoked_at": aggregate.last_invoked_at.map(|value| value.to_rfc3339()),
        "last_error_at": aggregate.last_error_at.map(|value| value.to_rfc3339()),
        "status": if aggregate.activations + aggregate.deactivations + aggregate.invocations + aggregate.errors > 0 { "available" } else { "missing" },
    })
}

fn value_json(aggregate: &Aggregate) -> Value {
    json!({
        "eval_runs": aggregate.eval_runs,
        "passed": aggregate.eval_passed,
        "failed": aggregate.eval_failed,
        "pass_rate": ratio(aggregate.eval_passed, aggregate.eval_runs),
        "baseline_delta_avg": mean(&aggregate.baseline_deltas),
        "status": if aggregate.eval_runs > 0 { "available" } else { "missing" },
    })
}

fn cost_json(aggregate: &Aggregate) -> Value {
    json!({
        "tokens_in": aggregate.tokens_in,
        "tokens_out": aggregate.tokens_out,
        "commands": aggregate.commands,
        "duration_ms": aggregate.duration_ms,
        "status": if aggregate.cost_seen { "available" } else { "missing" },
    })
}

fn sync_json() -> Value {
    json!({
        "uploaded_events": 0,
        "status": "not_instrumented",
    })
}

fn drift_json(aggregate: &Aggregate) -> Value {
    let stale_eval_days = aggregate.last_success_eval_at.map(|timestamp| {
        Utc::now()
            .signed_duration_since(timestamp)
            .num_days()
            .max(0)
    });
    json!({
        "stale_eval_days": stale_eval_days,
        "last_successful_eval_at": aggregate.last_success_eval_at.map(|value| value.to_rfc3339()),
        "status": if stale_eval_days.is_some() { "available" } else { "missing" },
    })
}

fn risk_json(aggregate: &Aggregate) -> Value {
    json!({
        "safety_events": aggregate.safety_events,
        "safety_findings": aggregate.safety_findings,
        "dependency_findings": aggregate.dependency_findings,
        "status": if aggregate.safety_events > 0 || aggregate.safety_findings > 0 || aggregate.dependency_findings > 0 { "available" } else { "missing" },
    })
}

fn feedback_json(aggregate: &Aggregate) -> Value {
    let total =
        aggregate.feedback_accepted + aggregate.feedback_rejected + aggregate.feedback_ignored;
    json!({
        "accepted": aggregate.feedback_accepted,
        "rejected": aggregate.feedback_rejected,
        "ignored": aggregate.feedback_ignored,
        "status": if total > 0 { "available" } else { "missing" },
    })
}

fn filtered_events<'a>(
    entries: &'a [TelemetryLogEntry],
    filters: &TelemetryFilters,
) -> Vec<&'a TelemetryLogEntry> {
    entries
        .iter()
        .filter(|entry| {
            filters
                .skill
                .as_deref()
                .is_none_or(|skill| entry.event.skill_id.as_deref() == Some(skill))
                && filters
                    .skillset
                    .as_deref()
                    .is_none_or(|skillset| entry.event.skillset_id.as_deref() == Some(skillset))
                && filters
                    .agent
                    .as_deref()
                    .is_none_or(|agent| entry.event.agent.as_deref() == Some(agent))
                && filters.workspace_hash.as_deref().is_none_or(|workspace| {
                    entry.event.workspace_hash.as_deref() == Some(workspace)
                })
                && filters
                    .since
                    .is_none_or(|since| entry.event.timestamp >= since)
        })
        .collect()
}

fn filters_json(filters: &TelemetryFilters) -> Value {
    json!({
        "skill": filters.skill,
        "skillset": filters.skillset,
        "agent": filters.agent,
        "workspace_hash": filters.workspace_hash,
        "since": filters.since.map(|value| value.to_rfc3339()),
    })
}

fn storage_json(ctx: &AppContext) -> Value {
    json!({
        "dir": telemetry_dir(ctx).display().to_string(),
        "config_path": config_path(ctx).display().to_string(),
        "events_path": events_path(ctx).display().to_string(),
    })
}

fn privacy_json(mode: &str) -> Value {
    json!({
        "mode": mode,
        "workspace_id": "hashed",
        "session_id": "hashed",
        "raw_prompt_stored": false,
        "raw_code_stored": false,
        "exports_redacted_by_default": true,
    })
}

fn malformed_json(lines: &[MalformedTelemetryLine]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| {
            json!({
                "line": line.line,
                "bytes": line.bytes,
                "error": line.error,
                "status": "quarantined",
            })
        })
        .collect()
}

fn malformed_warnings(lines: &[MalformedTelemetryLine]) -> Vec<String> {
    let mut warnings = lines
        .iter()
        .take(5)
        .map(|line| {
            format!(
                "telemetry event line {} quarantined: {}",
                line.line, line.error
            )
        })
        .collect::<Vec<_>>();
    if lines.len() > warnings.len() {
        warnings.push(format!(
            "{} additional malformed telemetry event(s) quarantined",
            lines.len() - warnings.len()
        ));
    }
    warnings
}

fn purge_plan(log: &TelemetryLog, before: Option<DateTime<Utc>>) -> PurgePlan {
    let matching = log
        .events
        .iter()
        .filter(|entry| event_before(entry, before))
        .collect::<Vec<_>>();
    let matching_events = matching.len();
    let matching_bytes = matching.iter().map(|entry| entry.bytes).sum::<usize>();
    PurgePlan {
        matching_events,
        matching_bytes,
        confirm_token: purge_token(before, matching_events, matching_bytes),
    }
}

fn event_before(entry: &TelemetryLogEntry, before: Option<DateTime<Utc>>) -> bool {
    before.is_none_or(|cutoff| entry.event.timestamp < cutoff)
}

fn max_timestamp(slot: &mut Option<DateTime<Utc>>, candidate: DateTime<Utc>) {
    if slot.is_none_or(|current| candidate > current) {
        *slot = Some(candidate);
    }
}

fn ratio(numerator: u64, denominator: u64) -> Option<f64> {
    if denominator == 0 {
        None
    } else {
        Some(numerator as f64 / denominator as f64)
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn current_workspace() -> std::result::Result<PathBuf, CommandFailure> {
    std::env::current_dir().map_err(map_io)
}
