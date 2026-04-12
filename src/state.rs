use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::gitops;
use crate::types::PendingOp;
const LOCK_STALE_AFTER: Duration = Duration::from_secs(60 * 60);
const OPS_COMPACTION_THRESHOLD: usize = 16;

#[derive(Debug, Clone)]
pub struct AppContext {
    pub root: PathBuf,
    pub skills_dir: PathBuf,
    pub state_dir: PathBuf,
    pub locks_dir: PathBuf,
    pub pending_ops_file: PathBuf,
    pub pending_ops_history_dir: PathBuf,
    pub pending_ops_snapshot_file: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct PendingOpsReport {
    pub ops: Vec<PendingOp>,
    pub warnings: Vec<String>,
    pub journal_events: usize,
    pub history_events: usize,
}

#[derive(Debug, Clone)]
pub struct AgentSkillDirs {
    pub claude: PathBuf,
    pub codex: PathBuf,
}

pub fn resolve_agent_skill_dirs(root: &Path) -> AgentSkillDirs {
    let home = env::var("HOME").unwrap_or_else(|_| "~".to_string());
    let dotenv = load_dotenv_map(root);

    let claude = env_or_dotenv("CLAUDE_SKILLS_DIR", &dotenv)
        .and_then(|raw| parse_dir_list_env(&raw).into_iter().next())
        .unwrap_or_else(|| PathBuf::from(format!("{}/.claude/skills", home)));

    let codex = env_or_dotenv("CODEX_SKILLS_DIR", &dotenv)
        .and_then(|raw| parse_dir_list_env(&raw).into_iter().next())
        .unwrap_or_else(|| PathBuf::from(format!("{}/.codex/skills", home)));

    AgentSkillDirs { claude, codex }
}

pub fn resolve_agent_skill_source_dirs(root: &Path) -> Vec<PathBuf> {
    let home = env::var("HOME").unwrap_or_else(|_| "~".to_string());
    let dotenv = load_dotenv_map(root);
    let mut dirs = Vec::new();

    if let Some(raw) = env_or_dotenv("CODEX_SKILLS_DIR", &dotenv) {
        dirs.extend(parse_dir_list_env(&raw));
    } else {
        dirs.push(PathBuf::from(format!("{}/.codex/skills", home)));
    }

    if let Some(raw) = env_or_dotenv("CLAUDE_SKILLS_DIR", &dotenv) {
        dirs.extend(parse_dir_list_env(&raw));
    } else {
        dirs.push(PathBuf::from(format!("{}/.claude/skills", home)));
    }

    dedupe_paths_keep_order(dirs)
}

fn env_or_dotenv(key: &str, dotenv: &BTreeMap<String, String>) -> Option<String> {
    env::var(key).ok().or_else(|| dotenv.get(key).cloned())
}

fn load_dotenv_map(root: &Path) -> BTreeMap<String, String> {
    let path = root.join(".env");
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return BTreeMap::new(),
    };

    let mut vars = BTreeMap::new();
    for line in raw.lines() {
        if let Some((key, value)) = parse_dotenv_line(line) {
            vars.insert(key, value);
        }
    }
    vars
}

fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, raw_value) = assignment.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }

    Some((key.to_string(), parse_dotenv_value(raw_value)))
}

fn parse_dotenv_value(raw: &str) -> String {
    let value = raw.trim();

    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        return value[1..value.len() - 1].replace("\\n", "\n");
    }

    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return value[1..value.len() - 1].to_string();
    }

    let without_comment = match value.split_once(" #") {
        Some((v, _)) => v.trim_end(),
        None => value,
    };

    without_comment.trim().to_string()
}

fn parse_dir_list_env(raw: &str) -> Vec<PathBuf> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn dedupe_paths_keep_order(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();

    for path in paths {
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            unique.push(path);
        }
    }

    unique
}

impl AppContext {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let root =
            root.unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);
        let skills_dir = root.join("skills");
        let state_dir = root.join("state");
        let locks_dir = state_dir.join("locks");
        let pending_ops_file = state_dir.join("pending_ops.jsonl");
        let pending_ops_history_dir = state_dir.join("pending_ops_history");
        let pending_ops_snapshot_file = state_dir.join("pending_ops_snapshot.json");

        Ok(Self {
            root,
            skills_dir,
            state_dir,
            locks_dir,
            pending_ops_file,
            pending_ops_history_dir,
            pending_ops_snapshot_file,
        })
    }

    pub fn ensure_state_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.skills_dir).context("failed to create skills directory")?;
        fs::create_dir_all(&self.locks_dir).context("failed to create state locks directory")?;
        fs::create_dir_all(&self.pending_ops_history_dir)
            .context("failed to create pending ops history directory")?;
        ensure_file_with_contents(&self.pending_ops_file, "")?;
        Ok(())
    }

    pub fn skill_path(&self, skill: &str) -> PathBuf {
        self.skills_dir.join(skill)
    }

    pub fn append_pending(
        &self,
        command: &str,
        details: serde_json::Value,
        request_id: String,
    ) -> Result<PendingOp> {
        self.ensure_state_layout()?;
        let op = PendingOp::new(command, details, request_id);
        let event = OpJournalEvent::Queued {
            event_id: new_event_id(),
            at: Utc::now(),
            op: op.clone(),
        };
        self.append_journal_events(&[event])?;
        self.maybe_compact_ops_journal()?;
        Ok(op)
    }

    pub fn read_pending_report(&self) -> Result<PendingOpsReport> {
        let model = self.read_ops_model()?;
        let mut ops = model.active_ops.into_values().collect::<Vec<_>>();
        ops.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(PendingOpsReport {
            ops,
            warnings: model.warnings,
            journal_events: model.journal_events,
            history_events: model.history_events,
        })
    }

    pub fn pending_count(&self) -> Result<usize> {
        Ok(self.read_pending_report()?.ops.len())
    }

    pub fn remove_pending_ops(&self, op_ids: &BTreeSet<String>) -> Result<usize> {
        self.ensure_state_layout()?;
        if op_ids.is_empty() {
            return Ok(0);
        }

        let model = self.read_ops_model()?;
        let removable = model
            .active_ops
            .keys()
            .filter(|op_id| op_ids.contains(*op_id))
            .cloned()
            .collect::<Vec<_>>();
        if removable.is_empty() {
            return Ok(0);
        }

        let events = removable
            .iter()
            .map(|op_id| OpJournalEvent::Removed {
                event_id: new_event_id(),
                at: Utc::now(),
                op_id: op_id.clone(),
                reason: "acked".to_string(),
            })
            .collect::<Vec<_>>();
        self.append_journal_events(&events)?;
        self.maybe_compact_ops_journal()?;
        Ok(removable.len())
    }

    pub fn purge_pending(&self) -> Result<usize> {
        self.ensure_state_layout()?;
        let model = self.read_ops_model()?;
        if model.active_ops.is_empty() {
            return Ok(0);
        }

        let events = model
            .active_ops
            .keys()
            .map(|op_id| OpJournalEvent::Removed {
                event_id: new_event_id(),
                at: Utc::now(),
                op_id: op_id.clone(),
                reason: "purged".to_string(),
            })
            .collect::<Vec<_>>();
        let purged = events.len();
        self.append_journal_events(&events)?;
        self.maybe_compact_ops_journal()?;
        Ok(purged)
    }

    pub fn lock_workspace(&self) -> Result<LockGuard> {
        self.lock_named("workspace")
    }

    pub fn lock_skill(&self, skill: &str) -> Result<LockGuard> {
        self.lock_named(&format!("skill-{}", skill))
    }

    fn lock_named(&self, name: &str) -> Result<LockGuard> {
        if is_loom_tool_repo_root(&self.root) {
            anyhow::bail!(
                "ARG_INVALID:refusing write operations in Loom tool repository root '{}'; use --root <separate skill registry repo>",
                self.root.display()
            );
        }
        self.ensure_state_layout()?;
        let lock_path = self.locks_dir.join(format!("{}.lock", name));

        for _ in 0..2 {
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    let metadata = LockMetadata::new();
                    let payload = serde_json::to_string(&metadata)
                        .context("failed to encode lock metadata")?;
                    writeln!(file, "{}", payload).context("failed to write lock file")?;
                    file.sync_all().context("failed to sync lock file")?;
                    return Ok(LockGuard { lock_path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if try_reap_stale_lock(&lock_path)? {
                        continue;
                    }
                    anyhow::bail!("LOCK_BUSY:{}", name);
                }
                Err(err) => return Err(err).context("failed to acquire lock"),
            }
        }

        anyhow::bail!("LOCK_BUSY:{}", name)
    }

    pub fn ensure_gitignore_entries(&self) -> Result<()> {
        let path = self.root.join(".gitignore");
        let mut content = if path.exists() {
            fs::read_to_string(&path).context("failed to read .gitignore")?
        } else {
            String::new()
        };

        let entries = [
            "state/locks/",
            "state/panel.log",
            "panel/node_modules/",
            "panel/dist/",
            "target/",
        ];

        for entry in entries {
            if !content.lines().any(|line| line.trim() == entry) {
                if !content.ends_with('\n') && !content.is_empty() {
                    content.push('\n');
                }
                content.push_str(entry);
                content.push('\n');
            }
        }

        write_atomic(&path, &content).context("failed to update .gitignore")?;
        Ok(())
    }

    fn read_ops_model(&self) -> Result<OpsReadModel> {
        let snapshot = self.load_ops_snapshot()?;
        let mut active_ops = snapshot
            .active_ops
            .into_iter()
            .map(|op| (op.stable_id(), op))
            .collect::<BTreeMap<_, _>>();
        let mut warnings = snapshot.warnings;
        let mut journal_events = 0usize;

        let file = match OpenOptions::new().read(true).open(&self.pending_ops_file) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(OpsReadModel {
                    active_ops,
                    warnings,
                    journal_events,
                    history_events: snapshot.history_events,
                });
            }
            Err(err) => return Err(err).context("failed to open pending_ops.jsonl"),
        };

        for (line_no, line) in BufReader::new(file).lines().enumerate() {
            let line = line.context("failed to read pending line")?;
            if line.trim().is_empty() {
                continue;
            }
            match parse_journal_line(&line) {
                Ok(event) => {
                    apply_journal_event(&mut active_ops, event);
                    journal_events += 1;
                }
                Err(err) => warnings.push(format!(
                    "skipped malformed pending op at line {}: {}",
                    line_no + 1,
                    err
                )),
            }
        }

        Ok(OpsReadModel {
            active_ops,
            warnings,
            journal_events,
            history_events: snapshot.history_events,
        })
    }

    fn load_ops_snapshot(&self) -> Result<LoadedSnapshot> {
        let raw = match fs::read_to_string(&self.pending_ops_snapshot_file) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedSnapshot::default());
            }
            Err(err) => return Err(err).context("failed to read pending ops snapshot"),
        };

        match serde_json::from_str::<OpsSnapshot>(&raw) {
            Ok(snapshot) => Ok(LoadedSnapshot {
                active_ops: snapshot.active_ops,
                history_events: snapshot.history_events,
                warnings: Vec::new(),
            }),
            Err(err) => Ok(LoadedSnapshot {
                active_ops: Vec::new(),
                history_events: 0,
                warnings: vec![format!("ignored malformed pending ops snapshot: {}", err)],
            }),
        }
    }

    fn append_journal_events(&self, events: &[OpJournalEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let lines = events
            .iter()
            .map(|event| serde_json::to_string(event).context("failed to encode pending op event"))
            .collect::<Result<Vec<_>>>()?;
        append_lines(&self.pending_ops_file, &lines)?;
        Ok(())
    }

    fn maybe_compact_ops_journal(&self) -> Result<()> {
        let raw_journal = match fs::read_to_string(&self.pending_ops_file) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).context("failed to read pending_ops.jsonl"),
        };
        let journal_event_count = raw_journal
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if journal_event_count < OPS_COMPACTION_THRESHOLD {
            return Ok(());
        }

        let model = self.read_ops_model()?;
        let segment_path = if !raw_journal.trim().is_empty() {
            let segment_path = self
                .pending_ops_history_dir
                .join(journal_segment_name(&raw_journal)?);
            write_history_segment_if_missing(&segment_path, &raw_journal)?;
            Some(segment_path)
        } else {
            None
        };
        maybe_fault_inject("ops_compact_after_history")?;

        let snapshot = OpsSnapshot {
            version: 1,
            created_at: Utc::now(),
            history_events: model.history_events + model.journal_events,
            active_ops: model.active_ops.into_values().collect(),
        };
        let snapshot_raw = serde_json::to_string_pretty(&snapshot)
            .context("failed to encode pending ops snapshot")?;
        write_atomic(&self.pending_ops_snapshot_file, &(snapshot_raw + "\n"))
            .context("failed to write pending ops snapshot")?;
        maybe_fault_inject("ops_compact_after_snapshot")?;
        if let Some(segment_path) = segment_path.as_ref() {
            gitops::mirror_history_segment(self, segment_path, &self.pending_ops_snapshot_file)
                .context("failed to mirror pending ops history into git")?;
        }
        write_atomic(&self.pending_ops_file, "").context("failed to compact pending_ops.jsonl")?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct LockGuard {
    lock_path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LockMetadata {
    pid: u32,
    created_at: DateTime<Utc>,
}

impl LockMetadata {
    fn new() -> Self {
        Self {
            pid: std::process::id(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum OpJournalEvent {
    Queued {
        event_id: String,
        at: DateTime<Utc>,
        op: PendingOp,
    },
    Removed {
        event_id: String,
        at: DateTime<Utc>,
        op_id: String,
        reason: String,
    },
}

impl OpJournalEvent {
    fn event_id(&self) -> &str {
        match self {
            Self::Queued { event_id, .. } | Self::Removed { event_id, .. } => event_id,
        }
    }

    fn at(&self) -> DateTime<Utc> {
        match self {
            Self::Queued { at, .. } | Self::Removed { at, .. } => *at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct OpsSnapshot {
    version: u32,
    created_at: DateTime<Utc>,
    history_events: usize,
    active_ops: Vec<PendingOp>,
}

#[derive(Debug, Default)]
struct LoadedSnapshot {
    active_ops: Vec<PendingOp>,
    history_events: usize,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct OpsReadModel {
    active_ops: BTreeMap<String, PendingOp>,
    warnings: Vec<String>,
    journal_events: usize,
    history_events: usize,
}

#[derive(Debug, Clone)]
pub struct HistoryBodySummary {
    pub first_at: Option<DateTime<Utc>>,
    pub last_at: Option<DateTime<Utc>>,
}

pub fn synthesize_snapshot_raw_from_segment_bodies(segment_bodies: &[String]) -> Result<String> {
    let mut seen_event_ids = BTreeSet::new();
    let mut ordered_events = Vec::new();

    for body in segment_bodies {
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event = parse_journal_line(trimmed)?;
            let event_id = event.event_id().to_string();
            if !seen_event_ids.insert(event_id.clone()) {
                continue;
            }
            ordered_events.push((event.at(), event_id, event));
        }
    }

    ordered_events.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));

    let mut active_ops = BTreeMap::new();
    for (_, _, event) in ordered_events {
        apply_journal_event(&mut active_ops, event);
    }

    let snapshot = OpsSnapshot {
        version: 1,
        created_at: Utc::now(),
        history_events: seen_event_ids.len(),
        active_ops: active_ops.into_values().collect(),
    };

    serde_json::to_string_pretty(&snapshot).context("failed to encode synthesized ops snapshot")
}

pub fn summarize_history_body(raw: &str) -> Result<HistoryBodySummary> {
    let mut first_at = None;
    let mut last_at = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = parse_journal_line(trimmed)?;
        let at = event.at();
        first_at = Some(first_at.map_or(at, |current: DateTime<Utc>| current.min(at)));
        last_at = Some(last_at.map_or(at, |current: DateTime<Utc>| current.max(at)));
    }

    Ok(HistoryBodySummary { first_at, last_at })
}

pub fn remove_path_if_exists(path: &Path) -> Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).context("failed to stat path"),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path).context("failed to remove file/symlink")?;
    } else {
        fs::remove_dir_all(path).context("failed to remove directory")?;
    }
    Ok(())
}

fn new_event_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn parse_journal_line(line: &str) -> Result<OpJournalEvent> {
    if let Ok(event) = serde_json::from_str::<OpJournalEvent>(line) {
        return Ok(event);
    }

    let mut op = serde_json::from_str::<PendingOp>(line)
        .context("line is neither a journal event nor a pending op")?;
    if op.op_id.is_none() {
        op.op_id = Some(op.stable_id());
    }
    Ok(OpJournalEvent::Queued {
        event_id: format!("legacy-{}", op.stable_id()),
        at: op.created_at,
        op,
    })
}

fn journal_segment_name(raw_journal: &str) -> Result<String> {
    let mut ids = Vec::new();
    let mut non_empty = 0usize;

    for (index, line) in raw_journal.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        non_empty += 1;
        match parse_journal_line(trimmed) {
            Ok(event) => ids.push(sanitize_segment_token(event.event_id())),
            Err(_) => ids.push(format!("invalid{}-{}", index + 1, trimmed.len())),
        }
    }

    if non_empty == 0 {
        return Err(anyhow::anyhow!("cannot name empty journal segment"));
    }

    let first = ids.first().cloned().unwrap_or_else(|| "empty".to_string());
    let last = ids.last().cloned().unwrap_or_else(|| "empty".to_string());
    Ok(format!(
        "{:05}-{}-{}.jsonl",
        non_empty,
        shorten_segment_token(&first),
        shorten_segment_token(&last)
    ))
}

fn apply_journal_event(active_ops: &mut BTreeMap<String, PendingOp>, event: OpJournalEvent) {
    match event {
        OpJournalEvent::Queued { mut op, .. } => {
            if op.op_id.is_none() {
                op.op_id = Some(op.stable_id());
            }
            active_ops.insert(op.stable_id(), op);
        }
        OpJournalEvent::Removed { op_id, .. } => {
            active_ops.remove(&op_id);
        }
    }
}

fn ensure_file_with_contents(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_atomic(path, contents).with_context(|| format!("failed to initialize {}", path.display()))
}

fn append_lines(path: &Path, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let parent = path
        .parent()
        .context("cannot append file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    for line in lines {
        writeln!(file, "{}", line)
            .with_context(|| format!("failed to append {}", path.display()))?;
    }
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))?;
    Ok(())
}

fn write_history_segment_if_missing(path: &Path, raw: &str) -> Result<()> {
    if raw.is_empty() {
        return Ok(());
    }

    match fs::read_to_string(path) {
        Ok(existing) => {
            let existing_normalized = if existing.ends_with('\n') {
                existing
            } else {
                format!("{}\n", existing)
            };
            let desired = if raw.ends_with('\n') {
                raw.to_string()
            } else {
                format!("{}\n", raw)
            };
            if existing_normalized == desired {
                return Ok(());
            }
            return Err(anyhow::anyhow!(
                "history segment already exists with different contents: {}",
                path.display()
            ));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    }

    let normalized = if raw.ends_with('\n') {
        raw.to_string()
    } else {
        format!("{}\n", raw)
    };
    write_atomic(path, &normalized)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn maybe_fault_inject(tag: &str) -> Result<()> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(anyhow::anyhow!("fault injected at {}", tag));
    }
    Ok(())
}

fn sanitize_segment_token(token: &str) -> String {
    token
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn shorten_segment_token(token: &str) -> String {
    token.chars().take(12).collect()
}

fn is_loom_tool_repo_root(root: &Path) -> bool {
    let manifest_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if canonicalize_or_self(root) == canonicalize_or_self(&manifest_root) {
        return true;
    }

    let cargo_toml = root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return false;
    }
    if !root.join("src/main.rs").exists() || !root.join("src/commands.rs").exists() {
        return false;
    }

    match fs::read_to_string(&cargo_toml) {
        Ok(content) => content.contains("name = \"skillloom\""),
        Err(_) => false,
    }
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("cannot write atomic file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;

    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));

    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file {}", tmp_path.display()))?;
    }

    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
}

fn try_reap_stale_lock(lock_path: &Path) -> Result<bool> {
    if is_lock_stale(lock_path)? {
        fs::remove_file(lock_path)
            .with_context(|| format!("failed to remove stale lock file {}", lock_path.display()))?;
        return Ok(true);
    }
    Ok(false)
}

fn is_lock_stale(lock_path: &Path) -> Result<bool> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(err).context("failed to read lock file"),
    };

    if let Ok(metadata) = serde_json::from_str::<LockMetadata>(raw.trim()) {
        let age = Utc::now().signed_duration_since(metadata.created_at);
        if let Ok(age) = age.to_std() {
            return Ok(age > LOCK_STALE_AFTER);
        }
    }

    let metadata = fs::metadata(lock_path).context("failed to stat lock file")?;
    let modified = match metadata.modified() {
        Ok(modified) => modified,
        Err(_) => return Ok(false),
    };
    let age = match modified.elapsed() {
        Ok(age) => age,
        Err(_) => return Ok(false),
    };
    Ok(age > LOCK_STALE_AFTER)
}
