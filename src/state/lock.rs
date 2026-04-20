use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const LOCK_STALE_AFTER: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct LockMetadata {
    pub(super) pid: u32,
    /// Hostname the lock was written from. Used to gate PID probes: across
    /// hosts (or across PID namespaces sharing the same workspace via a
    /// bind mount) `kill(pid, 0)` is meaningless and would wrongly report
    /// an alive holder as gone, so we fall back to time-based staleness.
    /// Default empty for backwards compatibility with lock files written
    /// before this field existed; `""` never equals a real hostname so the
    /// conservative branch is taken automatically.
    #[serde(default)]
    pub(super) host: String,
    pub(super) created_at: DateTime<Utc>,
}

impl LockMetadata {
    pub(super) fn new() -> Self {
        Self {
            pid: std::process::id(),
            host: current_hostname(),
            created_at: Utc::now(),
        }
    }
}

pub(super) fn try_reap_stale_lock(lock_path: &Path) -> Result<bool> {
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
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
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
}
