use std::fs::{self, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result, anyhow};

use super::resolve_git_index_path;
use crate::state::AppContext;

pub fn snapshot_index_to(ctx: &AppContext, backup_path: &Path) -> Result<()> {
    let index_path = resolve_git_index_path(ctx, &[])?;
    if !index_path.exists() {
        return Err(anyhow!(
            "git index missing at {}; cannot snapshot",
            index_path.display()
        ));
    }
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut source = fs::File::open(&index_path)
        .with_context(|| format!("open Git index {} for backup", index_path.display()))?;
    let source_permissions = source
        .metadata()
        .with_context(|| format!("read Git index metadata {}", index_path.display()))?
        .permissions();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        options.mode(source_permissions.mode());
    }
    let mut backup = options.open(backup_path).with_context(|| {
        format!(
            "create new Git index snapshot {}; refusing to follow or overwrite an existing entry",
            backup_path.display()
        )
    })?;
    backup
        .set_permissions(source_permissions)
        .with_context(|| format!("preserve permissions on {}", backup_path.display()))?;
    std::io::copy(&mut source, &mut backup).with_context(|| {
        format!(
            "back up Git index from {} to {}",
            index_path.display(),
            backup_path.display()
        )
    })?;
    backup
        .sync_all()
        .with_context(|| format!("sync Git index snapshot {}", backup_path.display()))?;
    Ok(())
}
