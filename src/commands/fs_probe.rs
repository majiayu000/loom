//! Runtime probe of what a projection target's filesystem can physically do.
//!
//! This is distinct from [`crate::state_model::V3TargetCapabilities`], which is
//! a *policy* capability declared at `target add` time (based on ownership).
//! A managed target always declares `capabilities.symlink = true`, but that
//! says nothing about whether the actual filesystem accepts the syscall:
//!
//! - Windows without Developer Mode or admin rejects `symlink_dir`.
//! - FAT32 / exFAT / some SMB mounts have no symlink concept.
//! - macOS TCC-restricted directories block the operation.
//!
//! Policy says "you may try"; the probe says "it actually works here".
//! Projection uses the probe result to decide whether to honour the requested
//! method or fall back.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

/// Whether the target filesystem physically supports symlinks, as measured
/// at projection time.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SymlinkProbe {
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl SymlinkProbe {
    pub fn supported() -> Self {
        Self {
            supported: true,
            reason: None,
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self {
            supported: false,
            reason: Some(reason.into()),
        }
    }
}

/// Try to create a throw-away symlink inside `parent` and report whether the
/// filesystem accepts the syscall. Creates `parent` if missing.
///
/// The probe points at a non-existent target: POSIX and Windows both allow
/// dangling symlinks, and we only care whether the syscall succeeds.
pub fn probe_symlink(parent: &Path) -> SymlinkProbe {
    if let Err(err) = fs::create_dir_all(parent) {
        return SymlinkProbe::unsupported(format!(
            "cannot create target parent {}: {}",
            parent.display(),
            err
        ));
    }

    let probe_path: PathBuf = parent.join(format!(".loom-probe-{}", Uuid::new_v4().simple()));
    let probe_target = parent.join(".loom-probe-target-does-not-exist");

    let result = create_symlink_probe(&probe_target, &probe_path);
    // Best-effort cleanup. Windows creates the probe via `symlink_dir`,
    // which produces a *directory* symlink: `remove_file` errors on those
    // entries and would leak `.loom-probe-*` on every successful probe.
    // `remove_symlink_probe` dispatches to the right API per platform.
    let _ = remove_symlink_probe(&probe_path);

    match result {
        Ok(()) => SymlinkProbe::supported(),
        Err(err) => SymlinkProbe::unsupported(format!("symlink not supported: {}", err)),
    }
}

#[cfg(unix)]
fn create_symlink_probe(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn create_symlink_probe(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

#[cfg(unix)]
fn remove_symlink_probe(path: &Path) -> std::io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_symlink_probe(path: &Path) -> std::io::Result<()> {
    fs::remove_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "loom-fs-probe-{}-{}",
            label,
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn unix_supports_symlink_on_tmp() {
        if !cfg!(unix) {
            return;
        }
        let dir = scratch_dir("supports");
        let probe = probe_symlink(&dir);
        assert!(probe.supported, "unix tmp should support symlink");
        assert!(probe.reason.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_cleans_up_after_itself() {
        let dir = scratch_dir("cleanup");
        let _ = probe_symlink(&dir);
        let leftover: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".loom-probe-"))
            .collect();
        assert!(leftover.is_empty(), "probe files must be cleaned up");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_creates_missing_parent() {
        let base = scratch_dir("mkparent");
        let deep = base.join("does/not/exist/yet");
        let probe = probe_symlink(&deep);
        if cfg!(unix) {
            assert!(probe.supported);
        }
        assert!(deep.exists(), "probe must create missing parent");
        let _ = fs::remove_dir_all(&base);
    }
}
