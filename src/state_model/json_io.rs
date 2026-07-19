//! Filesystem helpers for Registry state JSON/JSONL persistence.
//!
//! These are generic over any serde-serializable type; they don't know
//! anything about the Registry schema itself. Schema-aware orchestration (load
//! order, version validation, snapshot assembly) lives in
//! [`super::persistence`].

use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
use crate::fs_util::exchange_paths_atomic;
use crate::fs_util::{append_jsonl_raw, write_atomic};

const CAS_JOURNAL_MAGIC: &[u8; 8] = b"LOOMCAS1";

struct JsonFileLock {
    _file: fs::File,
}

impl JsonFileLock {
    fn acquire(path: &Path) -> std::io::Result<Self> {
        let parent = path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "JSON path has no parent")
        })?;
        let registry = if parent.file_name().is_some_and(|name| name == "registry") {
            Some(parent)
        } else {
            parent
                .parent()
                .filter(|path| path.file_name().is_some_and(|name| name == "registry"))
        };
        let lock_dir = if let Some(registry) = registry {
            registry.parent().unwrap_or(registry).join("locks")
        } else {
            parent.join(".loom-locks")
        };
        fs::create_dir_all(&lock_dir)?;
        let lock = lock_dir.join("registry-json.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock)?;
        file.lock()?;
        Ok(Self { _file: file })
    }
}

pub(super) fn ensure_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if path.exists() {
        ensure_existing_file(path)?;
        return Ok(());
    }
    write_json_file(path, value)
}

pub(super) fn ensure_text_file(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        ensure_existing_file(path)?;
        return Ok(());
    }
    Ok(write_atomic(path, contents)?)
}

fn ensure_existing_file(path: &Path) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    Err(anyhow!(
        "registry path exists but is not a file: {}",
        path.display()
    ))
}

pub(super) fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serialize_json_file(value)?;
    fs::create_dir_all(
        path.parent()
            .context("cannot write json file without parent")?,
    )?;
    let _lock = JsonFileLock::acquire(path)?;
    recover_json_cas(path)?;
    Ok(write_atomic(path, &raw)?)
}

/// Atomically replace a JSON file only when the displaced value still matches
/// the caller's reviewed baseline.
pub(super) fn compare_exchange_json_file<T>(
    path: &Path,
    expected: &T,
    replacement: &T,
) -> Result<bool>
where
    T: Serialize,
{
    let expected = serialize_json_file(expected)?;
    let raw = serialize_json_file(replacement)?;
    let parent = path
        .parent()
        .context("cannot replace json file without parent")?;
    fs::create_dir_all(parent)?;
    let _lock = JsonFileLock::acquire(path)?;
    recover_json_cas(path)?;
    let current = fs::read(path)?;
    let current_value = serde_json::from_slice::<serde_json::Value>(&current)?;
    let expected_value = serde_json::from_slice::<serde_json::Value>(expected.as_bytes())?;
    if current_value != expected_value {
        return Ok(false);
    }
    let candidate = path.with_extension("loom-cas-candidate");
    let journal = path.with_extension("loom-cas-journal");
    if candidate.exists() {
        return Err(anyhow!("untracked JSON CAS evidence retained"));
    }
    if current_value == serde_json::from_slice::<serde_json::Value>(raw.as_bytes())? {
        return Ok(true);
    }
    let expected = current;
    let journal_raw = encode_cas_journal(0, &expected, raw.as_bytes());
    crate::fs_util::write_atomic_bytes(&journal, &journal_raw)?;
    sync_parent(path)?;
    crate::fs_util::write_atomic_bytes(&candidate, raw.as_bytes())?;
    sync_parent(path)?;

    compare_exchange_json_candidate(path, &candidate)?;
    sync_parent(path)?;
    Ok(matches!(recover_json_cas(path)?, CasRecovery::Committed))
}

enum CasRecovery {
    None,
    Committed,
    Aborted,
}

fn encode_cas_journal(state: u8, expected: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(17 + expected.len() + replacement.len());
    raw.extend_from_slice(CAS_JOURNAL_MAGIC);
    raw.push(state);
    raw.extend_from_slice(&(expected.len() as u64).to_le_bytes());
    raw.extend_from_slice(expected);
    raw.extend_from_slice(replacement);
    raw
}

fn decode_cas_journal(raw: &[u8]) -> std::io::Result<(u8, &[u8], &[u8])> {
    if raw.len() < 17 || &raw[..8] != CAS_JOURNAL_MAGIC || raw[8] > 2 {
        return Err(std::io::Error::other("invalid JSON CAS journal header"));
    }
    let expected_len = usize::try_from(u64::from_le_bytes(raw[9..17].try_into().unwrap()))
        .map_err(|_| std::io::Error::other("invalid JSON CAS journal length"))?;
    let split = 17usize
        .checked_add(expected_len)
        .filter(|split| *split <= raw.len())
        .ok_or_else(|| std::io::Error::other("truncated JSON CAS journal"))?;
    Ok((raw[8], &raw[17..split], &raw[split..]))
}

fn recover_json_cas(path: &Path) -> std::io::Result<CasRecovery> {
    let journal = path.with_extension("loom-cas-journal");
    let raw = match fs::read(&journal) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(CasRecovery::None),
        Err(error) => return Err(error),
    };
    let (state, expected, replacement) = decode_cas_journal(&raw)?;
    if state == 0 {
        recover_json_cas_platform(path, &journal, expected, replacement)
    } else {
        finish_cas_decision(path, &journal, expected, replacement, state)
    }
}

fn record_cas_decision(
    path: &Path,
    journal: &Path,
    expected: &[u8],
    replacement: &[u8],
    outcome: CasRecovery,
) -> std::io::Result<CasRecovery> {
    let state = match outcome {
        CasRecovery::Committed => 1,
        CasRecovery::Aborted => 2,
        CasRecovery::None => unreachable!("a CAS decision cannot be none"),
    };
    crate::fs_util::write_atomic_bytes(journal, &encode_cas_journal(state, expected, replacement))?;
    sync_parent(path)?;
    finish_cas_decision(path, journal, expected, replacement, state)
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn finish_cas_recovery(path: &Path, journal: &Path, evidence: &Path) -> std::io::Result<()> {
    remove_if_exists(evidence)?;
    fs::remove_file(journal)?;
    sync_parent(path)
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> std::io::Result<()> {
    fs::File::open(path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "JSON path has no parent")
    })?)?
    .sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn compare_exchange_json_candidate(path: &Path, candidate: &Path) -> std::io::Result<()> {
    exchange_paths_atomic(candidate, path)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn recover_json_cas_platform(
    path: &Path,
    journal: &Path,
    expected: &[u8],
    replacement: &[u8],
) -> std::io::Result<CasRecovery> {
    let candidate = path.with_extension("loom-cas-candidate");
    let live = fs::read(path)?;
    let staged = match fs::read(&candidate) {
        Ok(staged) => Some(staged),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let outcome =
        if live == expected && staged.as_deref().is_none_or(|staged| staged == replacement) {
            CasRecovery::Aborted
        } else if live == replacement && staged.as_deref().is_none_or(|staged| staged == expected) {
            CasRecovery::Committed
        } else if staged.as_deref() == Some(replacement) {
            CasRecovery::Aborted
        } else {
            return Err(std::io::Error::other("ambiguous JSON CAS retained"));
        };
    record_cas_decision(path, journal, expected, replacement, outcome)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn finish_cas_decision(
    path: &Path,
    journal: &Path,
    expected: &[u8],
    replacement: &[u8],
    state: u8,
) -> std::io::Result<CasRecovery> {
    let candidate = path.with_extension("loom-cas-candidate");
    let owned = if state == 1 { expected } else { replacement };
    match fs::read(&candidate) {
        Ok(raw) if raw != owned => {
            return Err(std::io::Error::other("unknown JSON CAS evidence retained"));
        }
        _ => {}
    }
    finish_cas_recovery(path, journal, &candidate)?;
    Ok(if state == 1 {
        CasRecovery::Committed
    } else {
        CasRecovery::Aborted
    })
}

#[cfg(windows)]
fn compare_exchange_json_candidate(path: &Path, candidate: &Path) -> std::io::Result<()> {
    let backup = path.with_extension("loom-cas-backup");
    replace_file_with_backup_windows(path, candidate, &backup)?;
    Ok(())
}

#[cfg(windows)]
fn recover_json_cas_platform(
    path: &Path,
    journal: &Path,
    expected: &[u8],
    replacement: &[u8],
) -> std::io::Result<CasRecovery> {
    let candidate = path.with_extension("loom-cas-candidate");
    let backup = path.with_extension("loom-cas-backup");
    let live = fs::read(path)?;
    let staged = fs::read(&candidate).ok();
    let displaced = fs::read(&backup).ok();
    match (live.as_slice(), staged.as_deref(), displaced.as_deref()) {
        (live, Some(staged), None) if live == expected && staged == replacement => {
            record_cas_decision(path, journal, expected, replacement, CasRecovery::Aborted)
        }
        (live, None, None) if live == expected => {
            record_cas_decision(path, journal, expected, replacement, CasRecovery::Aborted)
        }
        (live, None, Some(old)) if live == replacement && old == expected => {
            record_cas_decision(path, journal, expected, replacement, CasRecovery::Committed)
        }
        (live, None, Some(old)) if live == replacement && old != expected => {
            replace_file_with_backup_windows(path, &backup, &candidate)?;
            recover_json_cas_platform(path, journal, expected, replacement)
        }
        (live, Some(staged), None) if live != expected && staged == replacement => {
            record_cas_decision(path, journal, expected, replacement, CasRecovery::Aborted)
        }
        _ => Err(std::io::Error::other("ambiguous JSON CAS retained")),
    }
}

#[cfg(windows)]
fn finish_cas_decision(
    path: &Path,
    journal: &Path,
    expected: &[u8],
    replacement: &[u8],
    state: u8,
) -> std::io::Result<CasRecovery> {
    let candidate = path.with_extension("loom-cas-candidate");
    let backup = path.with_extension("loom-cas-backup");
    let (evidence, owned) = if state == 1 {
        (&backup, expected)
    } else {
        (&candidate, replacement)
    };
    match fs::read(evidence) {
        Ok(raw) if raw != owned => {
            return Err(std::io::Error::other("unknown JSON CAS evidence retained"));
        }
        _ => {}
    }
    let unexpected = if state == 1 { &candidate } else { &backup };
    if unexpected.exists() {
        return Err(std::io::Error::other(
            "unexpected JSON CAS evidence retained",
        ));
    }
    finish_cas_recovery(path, journal, evidence)?;
    Ok(if state == 1 {
        CasRecovery::Committed
    } else {
        CasRecovery::Aborted
    })
}

#[cfg(windows)]
fn replace_file_with_backup_windows(
    path: &Path,
    replacement: &Path,
    backup: &Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const REPLACEFILE_WRITE_THROUGH: u32 = 0x1;
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let wide = |value: &Path| {
        value
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let path = wide(path);
    let replacement = wide(replacement);
    let backup = wide(backup);
    let ok = unsafe {
        ReplaceFileW(
            path.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    windows
)))]
fn compare_exchange_json_candidate(_path: &Path, candidate: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic JSON compare-and-exchange is unsupported",
    ))
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    windows
)))]
fn recover_json_cas_platform(
    path: &Path,
    _journal: &Path,
    _expected: &[u8],
    _replacement: &[u8],
) -> std::io::Result<CasRecovery> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "JSON CAS recovery is unsupported",
    ))
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    windows
)))]
fn finish_cas_decision(
    path: &Path,
    _journal: &Path,
    _expected: &[u8],
    _replacement: &[u8],
    _state: u8,
) -> std::io::Result<CasRecovery> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "JSON CAS recovery is unsupported",
    ))
}

pub(super) fn serialize_json_file<T: Serialize>(value: &T) -> Result<String> {
    let raw = serde_json::to_string_pretty(value).context("failed to encode registry json")?;
    Ok(raw + "\n")
}

/// Two-phase batch atomic write: write all temp files, then rename them all.
/// Minimizes the crash window for multi-file state updates. On a rename
/// failure midway, any temp files not yet renamed are cleaned up.
pub(super) fn write_atomic_batch(files: &[(&Path, &str)]) -> Result<()> {
    let lock = if let Some((path, _)) = files.first() {
        fs::create_dir_all(
            path.parent()
                .context("cannot write batch file without parent")?,
        )?;
        Some(JsonFileLock::acquire(path)?)
    } else {
        None
    };
    for (path, _) in files {
        recover_json_cas(path)?;
    }

    let mut staged: Vec<(PathBuf, &Path)> = Vec::with_capacity(files.len());

    // Phase 1: write all temp files
    for &(target, contents) in files {
        let parent = target
            .parent()
            .context("cannot write batch file without parent")?;
        fs::create_dir_all(parent)?;
        let tmp_path = parent.join(format!(
            ".{}.tmp-{}",
            target.file_name().unwrap_or_default().to_string_lossy(),
            uuid::Uuid::new_v4()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        staged.push((tmp_path, target));
    }

    // Phase 2: rename all (minimal crash window)
    for (tmp, target) in &staged {
        if let Err(err) = crate::fs_util::rename_atomic(tmp, target) {
            for (remaining, _) in &staged {
                let _ = fs::remove_file(remaining);
            }
            return Err(err)
                .with_context(|| format!("batch rename failed for {}", target.display()));
        }
    }

    drop(lock);
    Ok(())
}

pub(super) fn append_json_line<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serde_json::to_string(value)
        .with_context(|| format!("failed to encode registry jsonl line {}", path.display()))?;
    append_jsonl_raw(path, &raw)
        .with_context(|| format!("failed to append registry jsonl file {}", path.display()))
}

pub(super) fn write_json_lines<T>(path: &Path, values: &[T]) -> Result<()>
where
    T: Serialize,
{
    let mut raw = String::new();
    for value in values {
        raw.push_str(
            &serde_json::to_string(value).with_context(|| {
                format!("failed to encode registry jsonl line {}", path.display())
            })?,
        );
        raw.push('\n');
    }
    Ok(write_atomic(path, &raw)?)
}

pub(super) fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let _lock = JsonFileLock::acquire(path)?;
    recover_json_cas(path)?;
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read registry json file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse registry json file {}", path.display()))
}

pub(super) fn read_json_lines<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open registry jsonl file {}", path.display()))?;
    if file
        .metadata()
        .with_context(|| format!("failed to stat registry jsonl file {}", path.display()))?
        .len()
        == 0
    {
        return Ok(Vec::new());
    }
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read line {} from {}", index + 1, path.display())
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed to parse line {} from registry jsonl file {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn json_compare_exchange_installs_only_over_the_reviewed_value() {
        let root = std::env::temp_dir().join(format!("loom-json-cas-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create CAS fixture");
        let path = root.join("state.json");
        let reviewed = json!({"value": "reviewed"});
        let replacement = json!({"value": "replacement"});
        let external = json!({"value": "external"});
        write_json_file(&path, &reviewed).expect("write reviewed value");

        assert!(compare_exchange_json_file(&path, &reviewed, &replacement).expect("matching CAS"));
        assert_eq!(read_json_file::<Value>(&path).unwrap(), replacement);

        let candidate = path.with_extension("loom-cas-candidate");
        let journal = path.with_extension("loom-cas-journal");
        assert!(
            compare_exchange_json_file(&path, &replacement, &replacement)
                .expect("semantic no-op CAS")
        );
        assert_eq!(read_json_file::<Value>(&path).unwrap(), replacement);
        assert!(!candidate.exists());
        assert!(!journal.exists());

        fs::write(&candidate, b"untracked\n").expect("write stray CAS candidate");
        assert!(
            compare_exchange_json_file(&path, &replacement, &replacement).is_err(),
            "semantic no-op must not hide untracked CAS evidence"
        );
        assert!(candidate.exists());
        fs::remove_file(&candidate).expect("remove stray candidate");

        write_json_file(&path, &external).expect("write external value");
        assert!(
            !compare_exchange_json_file(&path, &reviewed, &replacement).expect("mismatching CAS")
        );
        assert_eq!(read_json_file::<Value>(&path).unwrap(), external);
        fs::remove_dir_all(root).expect("remove CAS fixture");
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    ))]
    #[test]
    fn cas_recovery_handles_each_unix_crash_boundary() {
        let root =
            std::env::temp_dir().join(format!("loom-json-cas-restore-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create CAS restore fixture");
        let path = root.join("state.json");
        let expected = serialize_json_file(&json!({"value": "reviewed"})).unwrap();
        let replacement = serialize_json_file(&json!({"value": "replacement"})).unwrap();
        let external = serialize_json_file(&json!({"value": "external"})).unwrap();
        let candidate = path.with_extension("loom-cas-candidate");
        let journal = path.with_extension("loom-cas-journal");

        let stage = |live: &str, staged: Option<&str>| {
            fs::write(&path, live).unwrap();
            fs::write(
                &journal,
                encode_cas_journal(0, expected.as_bytes(), replacement.as_bytes()),
            )
            .unwrap();
            if let Some(staged) = staged {
                fs::write(&candidate, staged).unwrap();
            }
        };

        stage(&expected, Some(&replacement));
        assert_eq!(
            read_json_file::<Value>(&path).unwrap(),
            json!({"value": "reviewed"})
        );
        assert!(!journal.exists() && !candidate.exists());

        stage(&replacement, Some(&expected));
        assert_eq!(
            read_json_file::<Value>(&path).unwrap(),
            json!({"value": "replacement"})
        );
        assert!(!journal.exists() && !candidate.exists());

        stage(&replacement, Some(&external));
        let retained_journal = fs::read(&journal).unwrap();
        let error = read_json_file::<Value>(&path).expect_err("unknown candidate must fail closed");
        assert!(error.to_string().contains("ambiguous JSON CAS retained"));
        assert_eq!(fs::read(&path).unwrap(), replacement.as_bytes());
        assert_eq!(fs::read(&candidate).unwrap(), external.as_bytes());
        assert_eq!(fs::read(&journal).unwrap(), retained_journal);
        assert!(read_json_file::<Value>(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), replacement.as_bytes());
        assert_eq!(fs::read(&candidate).unwrap(), external.as_bytes());
        assert_eq!(fs::read(&journal).unwrap(), retained_journal);

        fs::remove_file(&candidate).unwrap();
        fs::remove_file(&journal).unwrap();

        stage(&external, Some(&expected));
        let error =
            read_json_file::<Value>(&path).expect_err("ambiguous evidence must fail closed");
        assert!(error.to_string().contains("ambiguous JSON CAS retained"));
        assert_eq!(fs::read(&path).unwrap(), external.as_bytes());
        assert_eq!(fs::read(&candidate).unwrap(), expected.as_bytes());
        assert!(journal.exists());

        fs::remove_file(&candidate).unwrap();
        fs::write(
            &journal,
            encode_cas_journal(1, expected.as_bytes(), replacement.as_bytes()),
        )
        .unwrap();
        fs::write(&path, &replacement).unwrap();
        assert!(read_json_file::<Value>(&path).is_ok());
        assert!(!journal.exists());

        fs::write(
            &journal,
            encode_cas_journal(2, expected.as_bytes(), replacement.as_bytes()),
        )
        .unwrap();
        fs::write(&path, &external).unwrap();
        assert!(read_json_file::<Value>(&path).is_ok());
        assert!(!journal.exists());
        fs::remove_dir_all(root).expect("remove CAS restore fixture");
    }

    #[test]
    fn corrupt_cas_journal_is_retained_and_blocks_reads() {
        let root = std::env::temp_dir().join(format!("loom-json-cas-bad-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        fs::write(&path, "{}\n").unwrap();
        let journal = path.with_extension("loom-cas-journal");
        fs::write(&journal, b"partial").unwrap();

        let error = read_json_file::<Value>(&path).expect_err("corrupt journal must fail closed");
        assert!(error.to_string().contains("invalid JSON CAS journal"));
        assert_eq!(fs::read(&path).unwrap(), b"{}\n");
        assert_eq!(fs::read(&journal).unwrap(), b"partial");
        fs::remove_dir_all(root).unwrap();
    }
}
