mod ops;

pub use ops::{
    remove_path_if_exists, summarize_history_body, synthesize_snapshot_raw_from_segment_bodies,
};

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
    in_proc: Arc<Mutex<HashMap<String, (PathBuf, usize)>>>,
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
            in_proc: Arc::new(Mutex::new(HashMap::new())),
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

        // Fast path: same-process reentrant acquire via ref-count table.
        {
            let mut map = self.in_proc.lock().expect("in_proc mutex poisoned");
            if let Some((_path, count)) = map.get_mut(name) {
                *count += 1;
                return Ok(LockGuard {
                    name: name.to_string(),
                    in_proc: Arc::clone(&self.in_proc),
                });
            }
        }

        // Slow path: first acquire — attempt filesystem lock.
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
                    self.in_proc
                        .lock()
                        .expect("in_proc mutex poisoned")
                        .insert(name.to_string(), (lock_path, 1));
                    return Ok(LockGuard {
                        name: name.to_string(),
                        in_proc: Arc::clone(&self.in_proc),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if try_reap_stale_lock(&self.locks_dir.join(format!("{}.lock", name)))? {
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
}

#[derive(Debug)]
pub struct LockGuard {
    name: String,
    in_proc: Arc<Mutex<HashMap<String, (PathBuf, usize)>>>,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let mut map = match self.in_proc.lock() {
            Ok(m) => m,
            Err(_) => {
                eprintln!(
                    "loom: in_proc lock poisoned during drop for '{}'",
                    self.name
                );
                return;
            }
        };
        if let Some((lock_path, count)) = map.get_mut(&self.name) {
            *count -= 1;
            if *count == 0 {
                let lock_path = lock_path.clone();
                map.remove(&self.name);
                drop(map);
                if let Err(err) = fs::remove_file(&lock_path) {
                    eprintln!(
                        "loom: failed to release lock {}: {}",
                        lock_path.display(),
                        err
                    );
                }
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct LockMetadata {
    pid: u32,
    /// Hostname the lock was written from. Used to gate PID probes: across
    /// hosts (or across PID namespaces sharing the same workspace via a
    /// bind mount) `kill(pid, 0)` is meaningless and would wrongly report
    /// an alive holder as gone, so we fall back to time-based staleness.
    /// Default empty for backwards compatibility with lock files written
    /// before this field existed; `""` never equals a real hostname so the
    /// conservative branch is taken automatically.
    #[serde(default)]
    host: String,
    created_at: DateTime<Utc>,
}

impl LockMetadata {
    fn new() -> Self {
        Self {
            pid: std::process::id(),
            host: current_hostname(),
            created_at: Utc::now(),
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
        // PID probes are only meaningful when the lock was written from
        // the current host. Across hosts or PID namespaces (e.g. two
        // containers sharing a bind-mounted workspace), `kill(pid, 0)`
        // only sees the caller's own namespace — a still-running holder
        // in another namespace would appear as ESRCH and the lock would
        // be reaped out from under them. Lock files predating the `host`
        // field deserialize `host = ""`, which never matches a real
        // hostname, so they also take the conservative branch.
        let host = current_hostname();
        if !metadata.host.is_empty() && metadata.host == host {
            // Holder definitely gone — reap immediately. Any other outcome
            // (alive *or* indeterminate) falls through to the time-based
            // check below so a crashed process that left a stale PID
            // record still gets reaped after LOCK_STALE_AFTER. We
            // deliberately do NOT treat indeterminate probes as "dead" —
            // that previously deleted live locks on Windows and in
            // environments without `kill` on PATH.
            if let Some(false) = pid_status(metadata.pid) {
                return Ok(true);
            }
        }
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

/// Best-effort hostname probe.
///
/// Used to gate PID liveness checks so we never try to interpret
/// `kill(pid, 0)` results against a lock written on a different host or in
/// a different PID namespace. On failure we return `""`, which compares
/// unequal to every real hostname and therefore forces the conservative
/// time-based staleness branch — never matching "looks alive" or "looks
/// dead" from another namespace.
fn current_hostname() -> String {
    #[cfg(unix)]
    {
        // libc::gethostname wants a buffer; 256 bytes covers HOST_NAME_MAX
        // on every platform we target (Linux: 64, macOS: 255, POSIX: 255).
        let mut buf = [0u8; 256];
        // SAFETY: buf is valid for writes up to buf.len(), and gethostname
        // NUL-terminates on success.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut _, buf.len()) };
        if rc != 0 {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
    #[cfg(not(unix))]
    {
        // Windows exposes the machine name via COMPUTERNAME; fall back to
        // HOSTNAME (some shells set it) before giving up.
        env::var("COMPUTERNAME")
            .or_else(|_| env::var("HOSTNAME"))
            .unwrap_or_default()
    }
}

/// Probes whether a PID is still alive.
///
/// Returns `Some(true)` when the process is definitely alive, `Some(false)` when it
/// is definitely gone, and `None` when the probe was indeterminate (e.g. running on
/// a platform we cannot probe, or the syscall failed for an unexpected reason).
///
/// Callers MUST treat `None` as "unknown — do not reap on this evidence alone".
/// Callers MUST also gate this with a hostname check: a fresh `Some(false)`
/// from another host/namespace is meaningless (see `is_lock_stale`).
fn pid_status(pid: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only checks for existence; no signal is delivered.
        let res = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if res == 0 {
            Some(true)
        } else {
            match std::io::Error::last_os_error().raw_os_error() {
                // No such process — definitely dead.
                Some(libc::ESRCH) => Some(false),
                // Process exists but we lack permission to signal it (different uid).
                // Crucially, this means the holder is *alive*, not dead — never reap.
                Some(libc::EPERM) => Some(true),
                // Anything else (EINVAL, etc.) is unexpected; refuse to guess.
                _ => None,
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        // No portable cheap probe on non-unix without pulling in a winapi dep.
        // Returning None forces the caller into the time-based staleness branch,
        // which is the safer default than guessing alive/dead.
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: the previous `is_pid_alive` returned `false` for *any*
    /// non-zero `kill -0` exit (including EPERM or missing `kill` binary),
    /// causing live locks to be reaped. The current process must always
    /// be reported as alive on Unix.
    #[cfg(unix)]
    #[test]
    fn pid_status_reports_self_alive() {
        assert_eq!(pid_status(std::process::id()), Some(true));
    }

    /// Contract: non-Unix builds have no cheap probe, so `pid_status` returns
    /// `None` and callers MUST fall through to time-based staleness. Asserting
    /// `Some(true)` here on Windows would misrepresent the API.
    #[cfg(not(unix))]
    #[test]
    fn pid_status_is_indeterminate_on_non_unix() {
        assert_eq!(pid_status(std::process::id()), None);
    }

    /// Regression: on the current host, `current_hostname()` must return a
    /// non-empty string so `is_lock_stale` can actually reach the PID probe
    /// branch — an empty hostname would force every lock to the time-based
    /// fallback and defeat same-host crash detection.
    #[test]
    fn current_hostname_is_non_empty() {
        assert!(
            !current_hostname().is_empty(),
            "current_hostname() must resolve on the test runner"
        );
    }

    /// Regression for PR #1 P1 follow-up: locks written on a different host
    /// (or a different PID namespace) must NOT be reaped from a PID probe.
    /// We forge a lock record with a foreign `host` field and a PID that
    /// definitely does not exist on the local host, and assert that the
    /// stale check returns false because it falls through to the
    /// time-based branch before LOCK_STALE_AFTER elapses.
    #[test]
    fn cross_host_lock_is_not_reaped_by_pid_probe() {
        use std::fs;
        let dir =
            std::env::temp_dir().join(format!("loom-cross-host-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        let lock_path = dir.join("test.lock");
        let foreign = LockMetadata {
            // A PID that the current host almost certainly does not know;
            // on the *foreign* host it may well be alive. The point is
            // that we MUST NOT probe: cross-host probes are meaningless.
            pid: 999_999_999,
            host: String::from("foreign-host-that-is-not-us"),
            created_at: Utc::now(),
        };
        fs::write(&lock_path, serde_json::to_string(&foreign).unwrap()).unwrap();

        let stale = is_lock_stale(&lock_path).expect("stale probe must succeed");
        assert!(
            !stale,
            "fresh cross-host lock must not be reaped via PID probe"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reentrant_lock_succeeds() {
        let dir =
            std::env::temp_dir().join(format!("loom-reentrant-{}", uuid::Uuid::new_v4().simple()));
        let ctx = AppContext::new(Some(dir.clone())).unwrap();
        let guard1 = ctx.lock_workspace().expect("first lock must succeed");
        let guard2 = ctx
            .lock_workspace()
            .expect("second reentrant lock must succeed");
        let lock_path = ctx.locks_dir.join("workspace.lock");
        assert!(
            lock_path.exists(),
            "lock file must exist while guards are held"
        );
        drop(guard1);
        drop(guard2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inner_drop_does_not_release_file() {
        let dir =
            std::env::temp_dir().join(format!("loom-inner-drop-{}", uuid::Uuid::new_v4().simple()));
        let ctx = AppContext::new(Some(dir.clone())).unwrap();
        let guard1 = ctx.lock_workspace().unwrap();
        let guard2 = ctx.lock_workspace().unwrap();
        let lock_path = ctx.locks_dir.join("workspace.lock");
        drop(guard2);
        assert!(
            lock_path.exists(),
            "lock file must exist after inner guard drop"
        );
        drop(guard1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outer_drop_releases_file() {
        let dir =
            std::env::temp_dir().join(format!("loom-outer-drop-{}", uuid::Uuid::new_v4().simple()));
        let ctx = AppContext::new(Some(dir.clone())).unwrap();
        let guard1 = ctx.lock_workspace().unwrap();
        let guard2 = ctx.lock_workspace().unwrap();
        let lock_path = ctx.locks_dir.join("workspace.lock");
        drop(guard2);
        drop(guard1);
        assert!(
            !lock_path.exists(),
            "lock file must not exist after all guards dropped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cross_context_lock_is_busy() {
        let dir =
            std::env::temp_dir().join(format!("loom-cross-ctx-{}", uuid::Uuid::new_v4().simple()));
        let ctx_a = AppContext::new(Some(dir.clone())).unwrap();
        let ctx_b = AppContext::new(Some(dir.clone())).unwrap();
        let _guard = ctx_a.lock_workspace().expect("context A must acquire lock");
        let result = ctx_b.lock_workspace();
        assert!(
            result.is_err(),
            "context B must not acquire lock while A holds it"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("LOCK_BUSY"),
            "error must indicate LOCK_BUSY, got: {}",
            err_msg
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
