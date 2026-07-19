use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};

use crate::sha256::{Sha256, to_hex};

use super::prepared_index::path_entry_exists;
use super::{AppContext, resolve_git_index_path};

pub(super) fn prepared_index_aux_path(
    ctx: &AppContext,
    prepared_index: &Path,
    suffix: &str,
) -> Result<PathBuf> {
    let index = resolve_git_index_path(ctx, &[])?;
    let parent = index
        .parent()
        .ok_or_else(|| anyhow!("Git index has no parent: {}", index.display()))?;
    let identity = prepared_index_identity(prepared_index);
    Ok(parent.join(format!(".loom-index-{}{suffix}", &identity[..32])))
}

pub fn prepared_index_claim_exists(ctx: &AppContext, prepared_index: &Path) -> Result<bool> {
    path_entry_exists(&prepared_index_aux_path(
        ctx,
        prepared_index,
        ".lock-claim",
    )?)
}

fn prepared_index_identity(path: &Path) -> String {
    let mut hasher = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hasher.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            hasher.update(&unit.to_le_bytes());
        }
    }
    #[cfg(not(any(unix, windows)))]
    hasher.update(path.to_string_lossy().as_bytes());
    to_hex(&hasher.finalize())
}
