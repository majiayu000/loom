use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::state_model::RegistryStatePaths;

pub(super) fn maybe_projection_fault(tag: &str) -> Result<()> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(anyhow::anyhow!("fault injected at {}", tag));
    }
    Ok(())
}

pub(super) fn rollback_record_registry_operation(
    paths: &RegistryStatePaths,
    operations_len: u64,
    checkpoint_backup: &[u8],
) -> Result<()> {
    truncate_file(&paths.operations_file, operations_len)?;
    restore_raw_file(&paths.checkpoint_file, checkpoint_backup)?;
    Ok(())
}

fn truncate_file(path: &Path, len: u64) -> Result<()> {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open file for rollback {}", path.display()))?;
    file.set_len(len)
        .with_context(|| format!("failed to truncate file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync truncated file {}", path.display()))?;
    Ok(())
}

fn restore_raw_file(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("cannot restore file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create restore dir {}", parent.display()))?;
    fs::write(path, contents)
        .with_context(|| format!("failed to restore file {}", path.display()))?;
    Ok(())
}
