mod claude;
mod codex;
mod cursor;
mod plan;
mod source_file;
mod stream;

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File, Metadata};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::cli::{TelemetryIngestAgent, TelemetryIngestArgs};
use crate::envelope::Meta;
use crate::error_actions::NextAction;
use crate::state::{AppContext, home_dir};
use crate::types::ErrorCode;

use super::super::helpers::{map_io, map_lock, map_registry_state};
use super::super::{App, CommandFailure, build_skill_read_model};
use super::model::{TelemetryEventDraft, TelemetryEventType};
use super::store::{
    append_events_deduped_locked, observed_skill_name_allowed, parse_cutoff, read_config,
    session_hash_for_text,
};
use cursor::{IngestCursor, SourceCheckpoint};

const MAX_CAS_ATTEMPTS: usize = 50;
type SkillSummary = BTreeMap<(String, String), usize>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Agent {
    Claude,
    Codex,
}

impl Agent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug)]
pub(super) struct ImportedInvocation {
    name: String,
    identity: String,
    ordinal: usize,
}

#[derive(Debug)]
pub(super) struct ImportedRecord {
    stable_record_key: String,
    session_id: String,
    workspace: Option<PathBuf>,
    timestamp: DateTime<Utc>,
    invocations: Vec<ImportedInvocation>,
    rejected_reasons: Vec<&'static str>,
}

struct EventIdentityInput<'a> {
    agent: Agent,
    session_hash: &'a str,
    skill_name: &'a str,
    timestamp: DateTime<Utc>,
    logical_source_key: &'a str,
    stable_record_key: &'a str,
    invocation_identity: &'a str,
    ordinal: usize,
}

#[derive(Debug)]
pub(super) enum ParseOutcome {
    Ignored,
    Rejected(&'static str),
    Record(ImportedRecord),
}

#[derive(Default)]
struct ParserState {
    codex: codex::Context,
}

#[derive(Debug, Clone, Default, Serialize)]
struct AgentStats {
    scanned_files: usize,
    scanned_events: usize,
    ingested: usize,
    duplicates_skipped: usize,
    window_skipped: usize,
    malformed: usize,
    pending_partial: usize,
    rejected: usize,
}

struct SourcePlan {
    agent: Agent,
    source_key: String,
    source_guards: Vec<SourceGuard>,
    expected: Option<SourceCheckpoint>,
    checkpoint: SourceCheckpoint,
    drafts: Vec<TelemetryEventDraft>,
    authority: SourceAuthority,
    reset_reason: Option<cursor::ResetReason>,
    stats: AgentStats,
    rejected_reasons: BTreeMap<String, usize>,
}

enum ScanSourceOutcome {
    Planned(Box<SourcePlan>),
    Rediscover,
}

struct SourceGuard {
    source_path: PathBuf,
    snapshot: cursor::SourceSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SourceAuthority {
    len: u64,
    modified_nanos: u128,
    path_rank: String,
}

struct ScanPlan {
    agents: Vec<Agent>,
    stats: BTreeMap<Agent, AgentStats>,
    sources: Vec<SourcePlan>,
    reset_reasons: BTreeMap<String, usize>,
    rejected_reasons: BTreeMap<String, usize>,
    matched: SkillSummary,
    unmatched: SkillSummary,
    since: Option<DateTime<Utc>>,
}

enum CommitOutcome {
    Retry,
    Committed {
        appended_by_agent: BTreeMap<Agent, usize>,
        duplicates_by_agent: BTreeMap<Agent, usize>,
        matched: SkillSummary,
        unmatched: SkillSummary,
        cursor_advanced: bool,
    },
}

impl App {
    pub(super) fn cmd_telemetry_ingest(
        &self,
        args: &TelemetryIngestArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        let since = parse_cutoff("--since", args.since.as_deref())?;
        let agents = selected_agents(args.agent);
        if !args.dry_run {
            ensure_ingest_enabled(&self.ctx)?;
        }
        for _attempt in 0..MAX_CAS_ATTEMPTS {
            let mut plan = scan_all(&self.ctx, &agents, since)?;
            if args.dry_run {
                plan::preview_dedupe(&self.ctx, &mut plan)?;
                return Ok((plan::plan_json(&plan, true, false)?, Meta::default()));
            }
            #[cfg(debug_assertions)]
            debug_pause("LOOM_TEST_INGEST_COMMIT_PAUSE_MS");
            match commit_plan(&self.ctx, &plan)? {
                CommitOutcome::Retry => {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                CommitOutcome::Committed {
                    appended_by_agent,
                    duplicates_by_agent,
                    matched,
                    unmatched,
                    cursor_advanced,
                } => {
                    plan.matched = matched;
                    plan.unmatched = unmatched;
                    for agent in &agents {
                        let stats = plan.stats.entry(*agent).or_default();
                        stats.ingested = appended_by_agent.get(agent).copied().unwrap_or_default();
                        stats.duplicates_skipped =
                            duplicates_by_agent.get(agent).copied().unwrap_or_default();
                    }
                    return Ok((
                        plan::plan_json(&plan, false, cursor_advanced)?,
                        Meta::default(),
                    ));
                }
            }
        }
        Err(CommandFailure::new(
            ErrorCode::LockBusy,
            "telemetry ingest cursor changed during all compare-and-commit retries",
        ))
    }
}

fn selected_agents(selected: TelemetryIngestAgent) -> Vec<Agent> {
    match selected {
        TelemetryIngestAgent::Claude => vec![Agent::Claude],
        TelemetryIngestAgent::Codex => vec![Agent::Codex],
        TelemetryIngestAgent::All => vec![Agent::Claude, Agent::Codex],
    }
}

fn ensure_ingest_enabled(ctx: &AppContext) -> std::result::Result<(), CommandFailure> {
    if read_config(ctx)?.is_some_and(|config| config.enabled) {
        return Ok(());
    }
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        "telemetry ingest requires local telemetry to be enabled",
    );
    failure.next_actions.push(NextAction::new(
        "loom telemetry enable --local-only --json",
        "enable the redacted local telemetry store before ingesting agent logs",
    ));
    Err(failure)
}

fn scan_all(
    ctx: &AppContext,
    agents: &[Agent],
    since: Option<DateTime<Utc>>,
) -> std::result::Result<ScanPlan, CommandFailure> {
    let inventory = build_skill_read_model(ctx).map_err(map_registry_state)?;
    let registered = inventory
        .skills
        .iter()
        .filter(|skill| skill["source_status"].as_str() == Some("present"))
        .filter_map(|skill| skill["skill_id"].as_str().map(str::to_string))
        .collect::<BTreeSet<_>>();
    let cursor = cursor::read_cursor(ctx)?;
    let mut plan = ScanPlan {
        agents: agents.to_vec(),
        stats: BTreeMap::new(),
        sources: Vec::new(),
        reset_reasons: BTreeMap::new(),
        rejected_reasons: BTreeMap::new(),
        matched: BTreeMap::new(),
        unmatched: BTreeMap::new(),
        since,
    };
    for agent in agents {
        scan_agent(ctx, *agent, since, &registered, &cursor, &mut plan)?;
    }
    plan::coalesce_sources(&mut plan)?;
    Ok(plan)
}

fn scan_agent(
    _ctx: &AppContext,
    agent: Agent,
    since: Option<DateTime<Utc>>,
    registered: &BTreeSet<String>,
    ingest_cursor: &IngestCursor,
    plan: &mut ScanPlan,
) -> std::result::Result<(), CommandFailure> {
    let home = resolve_agent_home(agent)?;
    for _attempt in 0..3 {
        let sources = discover_sources(agent, &home)?;
        let mut staged = Vec::new();
        let mut rediscover = false;
        for source in &sources {
            match scan_source(agent, &home, source, since, registered, ingest_cursor)? {
                ScanSourceOutcome::Planned(source_plan) => staged.push(*source_plan),
                ScanSourceOutcome::Rediscover => {
                    rediscover = true;
                    break;
                }
            }
        }
        if rediscover {
            continue;
        }
        plan.stats.entry(agent).or_default().scanned_files = sources.len();
        plan.sources.extend(staged);
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::LockBusy,
        "telemetry sources changed during all discovery retries",
    ))
}

fn scan_source(
    agent: Agent,
    home: &Path,
    source: &Path,
    since: Option<DateTime<Utc>>,
    registered: &BTreeSet<String>,
    ingest_cursor: &IngestCursor,
) -> std::result::Result<ScanSourceOutcome, CommandFailure> {
    for _attempt in 0..3 {
        match scan_source_once(agent, home, source, since, registered, ingest_cursor)? {
            Some(source_plan) => return Ok(ScanSourceOutcome::Planned(Box::new(source_plan))),
            None if !source.try_exists().map_err(map_io)? => {
                return Ok(ScanSourceOutcome::Rediscover);
            }
            None => {}
        }
    }
    Err(CommandFailure::new(
        ErrorCode::LockBusy,
        "telemetry source changed during all snapshot retries",
    ))
}

fn scan_source_once(
    agent: Agent,
    home: &Path,
    source: &Path,
    since: Option<DateTime<Utc>>,
    registered: &BTreeSet<String>,
    ingest_cursor: &IngestCursor,
) -> std::result::Result<Option<SourcePlan>, CommandFailure> {
    let canonical_source = match fs::canonicalize(source) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io(error)),
    };
    let mut file = match File::open(&canonical_source) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(map_io(error)),
    };
    #[cfg(debug_assertions)]
    debug_pause("LOOM_TEST_INGEST_OPEN_PAUSE_MS");
    let metadata = file.metadata().map_err(map_io)?;
    let generation_identity = cursor::source_generation_identity(&file, &metadata)?;
    let authority = source_file::authority(source, &metadata)?;
    let source_identity =
        source_file::canonical_identity(agent, home, &canonical_source, &mut file)?;
    let source_key = cursor::logical_source_key(agent.as_str(), &source_identity);
    let expected = ingest_cursor.sources.get(&source_key).cloned();
    let window =
        cursor::scan_file_window(&mut file, &generation_identity, expected.as_ref(), since)?;
    let mut drafts = Vec::new();
    let mut stats = AgentStats::default();
    let mut rejected_reasons = BTreeMap::new();
    let mut parser_state = source_file::parser_state_before(
        agent,
        &mut file,
        window.parser_context_offset,
        window.start,
    )?;
    file.seek(SeekFrom::Start(window.start)).map_err(map_io)?;
    let mut reader = BufReader::new(file);
    let mut raw = Vec::new();
    let mut committed_offset = window.start;
    let mut parser_context_offset = window.parser_context_offset;
    let mut pending_partial = false;
    loop {
        let (consumed, oversized) = match stream::read_record(&mut reader, &mut raw)? {
            stream::RecordStatus::Complete { consumed } => (consumed, false),
            stream::RecordStatus::Oversized { consumed } => (consumed, true),
            stream::RecordStatus::Partial { consumed } => {
                debug_assert!(consumed > 0);
                pending_partial = true;
                break;
            }
            stream::RecordStatus::Eof => break,
        };
        let record_start = committed_offset;
        committed_offset = committed_offset.checked_add(consumed).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "telemetry ingest committed offset overflow",
            )
        })?;
        if oversized {
            checked_field_add(&mut stats.scanned_events, 1, "scanned_events")?;
            checked_field_add(&mut stats.malformed, 1, "malformed")?;
            continue;
        }
        if raw.is_empty() {
            continue;
        }
        checked_field_add(&mut stats.scanned_events, 1, "scanned_events")?;
        let value: Value = match serde_json::from_slice(&raw) {
            Ok(value) => value,
            Err(_) => {
                checked_field_add(&mut stats.malformed, 1, "malformed")?;
                continue;
            }
        };
        if agent == Agent::Codex
            && value.get("type").and_then(Value::as_str) == Some("turn_context")
        {
            parser_context_offset = record_start;
        }
        match parse_agent_record(agent, &value, &mut parser_state) {
            ParseOutcome::Ignored => {}
            ParseOutcome::Rejected(reason) => {
                reject_source(&mut stats, &mut rejected_reasons, reason)?
            }
            ParseOutcome::Record(record) => {
                for reason in record.rejected_reasons {
                    reject_source(&mut stats, &mut rejected_reasons, reason)?;
                }
                if since.is_some_and(|cutoff| record.timestamp < cutoff) {
                    checked_field_add(
                        &mut stats.window_skipped,
                        record.invocations.len(),
                        "window_skipped",
                    )?;
                    continue;
                }
                for invocation in record.invocations {
                    if !observed_skill_name_allowed(&invocation.name) {
                        reject_source(
                            &mut stats,
                            &mut rejected_reasons,
                            "invalid_observed_skill_name",
                        )?;
                        continue;
                    }
                    let session_hash = session_hash_for_text(&record.session_id);
                    let matched = registered.contains(&invocation.name);
                    let mut draft = TelemetryEventDraft::new(TelemetryEventType::SkillInvocation);
                    draft.skill_id = matched.then(|| invocation.name.clone());
                    draft.observed_skill_name = (!matched).then(|| invocation.name.clone());
                    draft.agent = Some(agent.as_str().to_string());
                    draft.workspace = record.workspace.clone();
                    draft.session_id = Some(record.session_id.clone());
                    draft.timestamp = record.timestamp;
                    draft.event_id_override = Some(deterministic_event_id(EventIdentityInput {
                        agent,
                        session_hash: &session_hash,
                        skill_name: &invocation.name,
                        timestamp: record.timestamp,
                        logical_source_key: &source_key,
                        stable_record_key: &record.stable_record_key,
                        invocation_identity: &invocation.identity,
                        ordinal: invocation.ordinal,
                    }));
                    drafts.push(draft);
                }
            }
        }
    }
    if pending_partial {
        checked_field_add(&mut stats.pending_partial, 1, "pending_partial")?;
    }
    #[cfg(debug_assertions)]
    debug_pause("LOOM_TEST_INGEST_SCAN_PAUSE_MS");
    let snapshot = cursor::source_snapshot(reader.get_ref(), &generation_identity)?;
    if snapshot != window.snapshot {
        return Ok(None);
    }
    let checkpoint = cursor::checkpoint_for_snapshot(
        reader.get_mut(),
        &snapshot,
        committed_offset,
        parser_context_offset,
        window.covered_since,
    )?;
    if cursor::source_snapshot(reader.get_ref(), &generation_identity)? != snapshot {
        return Ok(None);
    }
    Ok(Some(SourcePlan {
        agent,
        source_key,
        source_guards: vec![SourceGuard {
            source_path: source.to_path_buf(),
            snapshot: snapshot.clone(),
        }],
        expected,
        checkpoint,
        drafts,
        authority,
        reset_reason: window.reset_reason,
        stats,
        rejected_reasons,
    }))
}

fn parse_agent_record(agent: Agent, value: &Value, state: &mut ParserState) -> ParseOutcome {
    match agent {
        Agent::Claude => claude::parse_record(value),
        Agent::Codex => codex::parse_record(value, &mut state.codex),
    }
}

fn resolve_agent_home(agent: Agent) -> std::result::Result<PathBuf, CommandFailure> {
    let (loom_key, native_key, suffix) = match agent {
        Agent::Claude => ("LOOM_CLAUDE_HOME", "CLAUDE_HOME", ".claude"),
        Agent::Codex => ("LOOM_CODEX_HOME", "CODEX_HOME", ".codex"),
    };
    env_path(loom_key)
        .or_else(|| env_path(native_key))
        .or_else(|| home_dir().map(|home| home.join(suffix)))
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("cannot resolve {} log home", agent.as_str()),
            )
        })
}

fn env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn discover_sources(
    agent: Agent,
    home: &Path,
) -> std::result::Result<Vec<PathBuf>, CommandFailure> {
    let Some(home_metadata) = metadata_if_exists(home)? else {
        return Ok(Vec::new());
    };
    if !home_metadata.is_dir() {
        return Err(map_io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "telemetry agent home is not a directory",
        )));
    }
    let mut sources = Vec::new();
    match agent {
        Agent::Claude => collect_jsonl(&home.join("projects"), &mut sources)?,
        Agent::Codex => {
            let history = home.join("history.jsonl");
            if metadata_if_exists(&history)?.is_some_and(|metadata| metadata.is_file()) {
                sources.push(history);
            }
            collect_jsonl(&home.join("sessions"), &mut sources)?;
        }
    }
    sources.sort();
    sources.dedup();
    Ok(sources)
}

fn collect_jsonl(root: &Path, out: &mut Vec<PathBuf>) -> std::result::Result<(), CommandFailure> {
    let Some(root_metadata) = metadata_if_exists(root)? else {
        return Ok(());
    };
    if !root_metadata.is_dir() {
        return Err(map_io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "telemetry log root is not a directory",
        )));
    }
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|err| map_io(std::io::Error::other(err)))?;
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|value| value.to_str()) != Some("jsonl")
        {
            continue;
        }
        out.push(entry.into_path());
    }
    Ok(())
}

fn metadata_if_exists(path: &Path) -> std::result::Result<Option<Metadata>, CommandFailure> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(map_io(error)),
    }
}

fn commit_plan(
    ctx: &AppContext,
    plan: &ScanPlan,
) -> std::result::Result<CommitOutcome, CommandFailure> {
    let _workspace = match ctx.lock_workspace() {
        Ok(workspace) => workspace,
        Err(err) if err.to_string().contains("LOCK_BUSY") => return Ok(CommitOutcome::Retry),
        Err(err) => return Err(map_lock(err)),
    };
    ensure_ingest_enabled(ctx)?;
    let mut current = cursor::read_cursor(ctx)?;
    if plan
        .sources
        .iter()
        .any(|source| current.sources.get(&source.source_key).cloned() != source.expected)
    {
        return Ok(CommitOutcome::Retry);
    }
    for source in &plan.sources {
        for guard in &source.source_guards {
            let file = match File::open(&guard.source_path) {
                Ok(file) => file,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(CommitOutcome::Retry);
                }
                Err(err) => return Err(map_io(err)),
            };
            let metadata = file.metadata().map_err(map_io)?;
            let generation_identity = cursor::source_generation_identity(&file, &metadata)?;
            if cursor::source_snapshot(&file, &generation_identity)? != guard.snapshot {
                return Ok(CommitOutcome::Retry);
            }
        }
    }
    let mut draft_agents = BTreeMap::<String, Agent>::new();
    let mut drafts = Vec::new();
    for source in &plan.sources {
        for draft in &source.drafts {
            if let Some(event_id) = draft.event_id_override.as_ref() {
                draft_agents.insert(event_id.clone(), source.agent);
            }
            drafts.push(draft.clone());
        }
    }
    let result = append_events_deduped_locked(ctx, drafts)?;
    let mut appended_by_agent = BTreeMap::new();
    for event in &result.appended {
        if let Some(agent) = draft_agents.get(&event.event_id) {
            checked_increment(&mut appended_by_agent, *agent)?;
        }
    }
    let mut candidates_by_agent = BTreeMap::new();
    for source in &plan.sources {
        checked_field_add(
            candidates_by_agent.entry(source.agent).or_default(),
            source.drafts.len(),
            "candidate events",
        )?;
    }
    let mut duplicates_by_agent = BTreeMap::new();
    for agent in &plan.agents {
        let candidates = candidates_by_agent.get(agent).copied().unwrap_or_default();
        let appended = appended_by_agent.get(agent).copied().unwrap_or_default();
        duplicates_by_agent.insert(*agent, candidates.saturating_sub(appended));
    }
    debug_assert_eq!(
        checked_map_sum(duplicates_by_agent.values().copied(), "duplicates")?,
        result.duplicates
    );
    let (matched, unmatched) = plan::summarize_events(&result.appended)?;
    let mut cursor_advanced = false;
    for source in &plan.sources {
        if current.sources.get(&source.source_key) != Some(&source.checkpoint) {
            cursor_advanced = true;
        }
        current
            .sources
            .insert(source.source_key.clone(), source.checkpoint.clone());
    }
    cursor::write_cursor_locked(ctx, &current)?;
    Ok(CommitOutcome::Committed {
        appended_by_agent,
        duplicates_by_agent,
        matched,
        unmatched,
        cursor_advanced,
    })
}

fn deterministic_event_id(input: EventIdentityInput<'_>) -> String {
    let ordinal = input.ordinal.to_string();
    let timestamp = input.timestamp.to_rfc3339();
    let digest = cursor::hash_fields(
        "loom.telemetry.import-event.v1",
        &[
            input.agent.as_str(),
            input.session_hash,
            input.skill_name,
            &timestamp,
            input.logical_source_key,
            input.stable_record_key,
            input.invocation_identity,
            &ordinal,
        ],
    );
    format!("evt_{}", digest.trim_start_matches("sha256:"))
}

fn checked_map_sum(
    mut values: impl Iterator<Item = usize>,
    field: &str,
) -> std::result::Result<usize, CommandFailure> {
    values.try_fold(0usize, |total, value| {
        total.checked_add(value).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("telemetry ingest {field} overflow"),
            )
        })
    })
}

fn reject_source(
    stats: &mut AgentStats,
    rejected_reasons: &mut BTreeMap<String, usize>,
    reason: &'static str,
) -> std::result::Result<(), CommandFailure> {
    checked_field_add(&mut stats.rejected, 1, "rejected")?;
    checked_increment(rejected_reasons, reason.to_string())
}

fn checked_increment<K: Ord + Clone>(
    counts: &mut BTreeMap<K, usize>,
    key: K,
) -> std::result::Result<(), CommandFailure> {
    checked_field_add(counts.entry(key).or_default(), 1, "counter")
}

#[cfg(debug_assertions)]
fn debug_pause(key: &str) {
    if let Ok(raw) = env::var(key)
        && let Ok(milliseconds) = raw.parse::<u64>()
    {
        std::thread::sleep(Duration::from_millis(milliseconds.min(2_000)));
    }
}

fn checked_field_add(
    slot: &mut usize,
    value: usize,
    field: &str,
) -> std::result::Result<(), CommandFailure> {
    *slot = slot.checked_add(value).ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::InternalError,
            format!("telemetry ingest {field} overflow"),
        )
    })?;
    Ok(())
}
