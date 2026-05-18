use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::state::AppContext;

use super::exec::{run_git, run_git_in_with_input};

pub fn diff_path(ctx: &AppContext, from: &str, to: &str, path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["diff", from, to, "--", &path_str])
}

pub fn fsck(ctx: &AppContext) -> Result<String> {
    run_git(ctx, &["fsck", "--no-progress"])
}

pub(crate) fn hash_object_file(ctx: &AppContext, path: &Path) -> Result<String> {
    let path_str = path.to_string_lossy();
    run_git(ctx, &["hash-object", "-w", &path_str])
}

pub(crate) fn hash_object_bytes(ctx: &AppContext, bytes: &[u8]) -> Result<String> {
    run_git_in_with_input(&ctx.root, &["hash-object", "-w", "--stdin"], bytes)
}

pub(crate) fn read_blob(ctx: &AppContext, blob: &str) -> Result<String> {
    run_git(ctx, &["cat-file", "-p", blob])
}

pub(crate) struct TempFile {
    pub(crate) path: PathBuf,
}

impl TempFile {
    pub(crate) fn new(prefix: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!("{}-{}", prefix, uuid::Uuid::new_v4()));
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
