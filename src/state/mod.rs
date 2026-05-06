mod lock;
mod ops;

pub use ops::{
    remove_path_if_exists, summarize_history_body, synthesize_snapshot_raw_from_segment_bodies,
};

use lock::{LockMetadata, try_reap_stale_lock};

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use crate::types::PendingOp;

const OPS_COMPACTION_THRESHOLD: usize = 16;
pub const DEFAULT_REGISTRY_DIR: &str = ".loom-registry";

type InProcMap = Arc<Mutex<HashMap<String, (PathBuf, std::thread::ThreadId, usize)>>>;

#[derive(Debug, Clone)]
pub struct AppContext {
    pub root: PathBuf,
    pub skills_dir: PathBuf,
    pub state_dir: PathBuf,
    pub locks_dir: PathBuf,
    pub pending_ops_file: PathBuf,
    pub pending_ops_history_dir: PathBuf,
    pub pending_ops_snapshot_file: PathBuf,
    in_proc: InProcMap,
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
        let root = match root {
            Some(root) => root,
            None => default_registry_root()?,
        };
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

    pub fn ensure_not_loom_tool_repo_root(&self) -> Result<()> {
        if is_loom_tool_repo_root(&self.root) {
            anyhow::bail!(
                "ARG_INVALID:refusing write operations in Loom tool repository root '{}'; use --root <separate skill registry repo>",
                self.root.display()
            );
        }
        Ok(())
    }

    fn lock_named(&self, name: &str) -> Result<LockGuard> {
        self.ensure_not_loom_tool_repo_root()?;
        self.ensure_state_layout()?;
        let lock_path = self.locks_dir.join(format!("{}.lock", name));
        let current_thread = std::thread::current().id();

        // Fast path: same-process same-thread reentrant acquire via ref-count table.
        // Reentrancy is scoped to the current thread so that concurrent threads
        // sharing the same Arc (e.g. cloned AppContext across panel requests) still
        // block at the filesystem layer rather than bypassing it.
        {
            let mut map = self.in_proc.lock().expect("in_proc mutex poisoned");
            if let Some((_path, holder, count)) = map.get_mut(name)
                && *holder == current_thread
            {
                *count += 1;
                return Ok(LockGuard {
                    name: name.to_string(),
                    in_proc: Arc::clone(&self.in_proc),
                });
                // If a different thread holds the entry, fall through to the
                // filesystem acquire which will fail AlreadyExists → LOCK_BUSY.
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
                        .insert(name.to_string(), (lock_path, current_thread, 1));
                    return Ok(LockGuard {
                        name: name.to_string(),
                        in_proc: Arc::clone(&self.in_proc),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    // If in_proc has an entry for this name, a different thread in this
                    // process holds the lock and the filesystem lock file is live.
                    // Skip stale-reaping — the lock is not stale, and reaping would
                    // overwrite the active ref-count entry while that guard is still live.
                    {
                        let map = self.in_proc.lock().expect("in_proc mutex poisoned");
                        if map.contains_key(name) {
                            anyhow::bail!("LOCK_BUSY:{}", name);
                        }
                    }
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

pub fn default_registry_root() -> Result<PathBuf> {
    home_dir()
        .map(|home| home.join(DEFAULT_REGISTRY_DIR))
        .context("HOME is not set; pass --root <registry>")
}

fn home_dir() -> Option<PathBuf> {
    non_empty_env_path("HOME").or_else(|| non_empty_env_path("USERPROFILE"))
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key).and_then(|value| {
        if value.as_os_str().is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}

#[derive(Debug)]
pub struct LockGuard {
    name: String,
    in_proc: InProcMap,
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
        if let Some((lock_path, _holder, count)) = map.get_mut(&self.name) {
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
    if !root.join("src/main.rs").exists()
        || (!root.join("src/commands.rs").exists() && !root.join("src/commands/mod.rs").exists())
    {
        return false;
    }

    package_name_from_cargo_toml(&cargo_toml).is_some_and(|name| name == "skillloom")
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn package_name_from_cargo_toml(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let mut in_package = false;
    for raw_line in content.lines() {
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(line, _)| line)
            .trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let value = value.trim();
        return value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .map(str::to_string);
    }
    None
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

    crate::fs_util::rename_atomic(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
