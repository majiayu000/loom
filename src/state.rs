use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::types::{PendingOp, TargetsState};

#[derive(Debug, Clone)]
pub struct AppContext {
    pub root: PathBuf,
    pub skills_dir: PathBuf,
    pub state_dir: PathBuf,
    pub locks_dir: PathBuf,
    pub pending_ops_file: PathBuf,
    pub targets_file: PathBuf,
}

impl AppContext {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let root =
            root.unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);
        let skills_dir = root.join("skills");
        let state_dir = root.join("state");
        let locks_dir = state_dir.join("locks");
        let pending_ops_file = state_dir.join("pending_ops.jsonl");
        let targets_file = state_dir.join("targets.json");

        fs::create_dir_all(&skills_dir).context("failed to create skills directory")?;
        fs::create_dir_all(&locks_dir).context("failed to create state locks directory")?;
        if !pending_ops_file.exists() {
            fs::write(&pending_ops_file, "").context("failed to create pending_ops.jsonl")?;
        }
        if !targets_file.exists() {
            fs::write(&targets_file, "{\n  \"skills\": {}\n}\n")
                .context("failed to create targets.json")?;
        }

        Ok(Self {
            root,
            skills_dir,
            state_dir,
            locks_dir,
            pending_ops_file,
            targets_file,
        })
    }

    pub fn skill_path(&self, skill: &str) -> PathBuf {
        self.skills_dir.join(skill)
    }

    pub fn load_targets(&self) -> Result<TargetsState> {
        let raw = fs::read_to_string(&self.targets_file).context("failed to read targets.json")?;
        let parsed =
            serde_json::from_str::<TargetsState>(&raw).context("failed to parse targets.json")?;
        Ok(parsed)
    }

    pub fn save_targets(&self, value: &TargetsState) -> Result<()> {
        let raw =
            serde_json::to_string_pretty(value).context("failed to serialize targets state")?;
        fs::write(&self.targets_file, raw + "\n").context("failed to write targets.json")?;
        Ok(())
    }

    pub fn append_pending(
        &self,
        command: &str,
        details: serde_json::Value,
        request_id: String,
    ) -> Result<()> {
        let op = PendingOp {
            request_id,
            command: command.to_string(),
            created_at: Utc::now(),
            details,
        };
        let line = serde_json::to_string(&op).context("failed to encode pending op")?;
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.pending_ops_file)
            .context("failed to open pending_ops.jsonl")?;
        writeln!(file, "{}", line).context("failed to append pending op")?;
        Ok(())
    }

    pub fn read_pending(&self) -> Result<Vec<PendingOp>> {
        let file = OpenOptions::new()
            .read(true)
            .open(&self.pending_ops_file)
            .context("failed to open pending_ops.jsonl")?;
        let reader = BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line.context("failed to read pending line")?;
            if line.trim().is_empty() {
                continue;
            }
            let op =
                serde_json::from_str::<PendingOp>(&line).context("failed to parse pending line")?;
            out.push(op);
        }
        Ok(out)
    }

    pub fn clear_pending(&self) -> Result<()> {
        fs::write(&self.pending_ops_file, "").context("failed to clear pending_ops")?;
        Ok(())
    }

    pub fn pending_count(&self) -> Result<usize> {
        Ok(self.read_pending()?.len())
    }

    pub fn lock_skill(&self, skill: &str) -> Result<SkillLockGuard> {
        let lock_path = self.locks_dir.join(format!("{}.lock", skill));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&lock_path)
        {
            Ok(mut file) => {
                writeln!(file, "pid={} req={}", std::process::id(), Uuid::new_v4())
                    .context("failed to write lock file")?;
                Ok(SkillLockGuard { lock_path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                anyhow::bail!("LOCK_BUSY:{}", skill)
            }
            Err(e) => Err(e).context("failed to acquire lock"),
        }
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

        fs::write(path, content).context("failed to update .gitignore")?;
        Ok(())
    }
}

pub struct SkillLockGuard {
    lock_path: PathBuf,
}

impl Drop for SkillLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
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
