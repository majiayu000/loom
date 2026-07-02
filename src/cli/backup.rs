use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use serde::Serialize;

#[derive(Debug, Clone, Subcommand, Serialize)]
pub enum BackupCommand {
    #[command(about = "Create a portable registry backup artifact")]
    Export(BackupExportArgs),
    #[command(about = "Inspect and validate a registry backup artifact")]
    Inspect(BackupInspectArgs),
    #[command(about = "Restore a registry backup into a new empty root")]
    Restore(BackupRestoreArgs),
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BackupExportArgs {
    /// Output tar path. Defaults to <root>/backups/loom-backup-<timestamp>.tar.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Backup artifact format.
    #[arg(long, value_enum, default_value_t = BackupFormat::Tar)]
    pub format: BackupFormat,

    /// Include registry-owned target cache data if present.
    #[arg(long)]
    pub include_target_cache: bool,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BackupInspectArgs {
    /// Backup artifact to inspect.
    pub artifact: PathBuf,
}

#[derive(Debug, Clone, Args, Serialize)]
pub struct BackupRestoreArgs {
    /// Backup artifact to restore.
    pub artifact: PathBuf,

    /// Permit a destination root that contains only safe empty scaffolding.
    #[arg(long)]
    pub force_empty_root: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackupFormat {
    Tar,
}
