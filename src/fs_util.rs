//! Cross-platform filesystem utilities.
//!
//! `std::fs::rename` on Windows fails with `ERROR_ALREADY_EXISTS` when the
//! destination path already exists. Every atomic-overwrite path (write-to-tmp
//! then replace) must use [`rename_atomic`] instead of `std::fs::rename`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::fd::AsRawFd;

/// Atomically replace `dst` with `src`.
///
/// On Unix this is a single `rename(2)` syscall, which is atomic at the
/// filesystem level. On Windows this uses `MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING` so the destination is replaced by the OS rather
/// than deleted first.
pub fn rename_atomic(src: &Path, dst: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return rename_atomic_windows(src, dst);

    #[cfg(not(windows))]
    std::fs::rename(src, dst)
}

/// Write contents through a temp file and atomically replace the destination.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot write atomic file without parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;

    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));

    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }

    rename_atomic(&tmp_path, path)
}

/// Append newline-terminated records and sync the file.
pub fn append_lines(path: &Path, lines: &[String]) -> io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }

    let mut file = open_append_file(path)?;
    for line in lines {
        let mut record = Vec::with_capacity(line.len() + 1);
        record.extend_from_slice(line.as_bytes());
        record.push(b'\n');
        append_single_record_write(&mut file, &record)?;
    }
    file.sync_all()
}

/// Append one pre-serialized JSONL record and sync the file.
pub fn append_jsonl_raw(path: &Path, raw: &str) -> io::Result<()> {
    let mut file = open_append_file(path)?;
    let mut record = Vec::with_capacity(raw.len() + 1);
    record.extend_from_slice(raw.as_bytes());
    record.push(b'\n');
    append_single_record_write(&mut file, &record)?;
    file.sync_all()
}

/// Ensure an append-only log file exists and is synced to disk.
pub fn ensure_append_log(path: &Path) -> io::Result<()> {
    open_append_file(path)?.sync_all()
}

pub fn maybe_fault_inject(tag: &str) -> io::Result<()> {
    if std::env::var("LOOM_FAULT_INJECT").ok().as_deref() == Some(tag) {
        return Err(io::Error::other(format!("fault injected at {}", tag)));
    }
    Ok(())
}

pub fn maybe_fault_inject_any(tags: &[&str]) -> io::Result<()> {
    let active = std::env::var("LOOM_FAULT_INJECT").ok();
    if let Some(tag) = active.as_deref().filter(|tag| tags.contains(tag)) {
        return Err(io::Error::other(format!("fault injected at {}", tag)));
    }
    Ok(())
}

pub fn remove_path_if_exists(path: &Path) -> io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path)
    } else {
        fs::remove_dir_all(path)
    }
}

fn open_append_file(path: &Path) -> io::Result<File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot append file without parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;
    OpenOptions::new().create(true).append(true).open(path)
}

#[cfg(unix)]
fn append_single_record_write(file: &mut File, record: &[u8]) -> io::Result<()> {
    if record.len() > isize::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "append record exceeds write size limit",
        ));
    }

    loop {
        // SAFETY: `record` points to a valid immutable byte buffer for
        // `record.len()` bytes, and `file.as_raw_fd()` is an open descriptor
        // owned by `file`.
        let written =
            unsafe { libc::write(file.as_raw_fd(), record.as_ptr().cast(), record.len()) };
        if written < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }

        let written = written as usize;
        if written == record.len() {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            format!(
                "short append record write: wrote {} of {} bytes",
                written,
                record.len()
            ),
        ));
    }
}

#[cfg(not(unix))]
fn append_single_record_write(file: &mut File, record: &[u8]) -> io::Result<()> {
    file.write_all(record)
}

#[cfg(windows)]
fn rename_atomic_windows(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    unsafe extern "system" {
        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    fn wide_null(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let src = wide_null(src);
    let dst = wide_null(dst);
    let ok = unsafe {
        MoveFileExW(
            src.as_ptr(),
            dst.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("loom-fs-util-{}-{}", label, Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn rename_atomic_overwrites_existing_file() {
        let dir = temp_dir("overwrite-file");
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");

        fs::write(&dst, b"old content").unwrap();
        fs::write(&src, b"new content").unwrap();

        rename_atomic(&src, &dst).expect("rename_atomic must succeed when dst exists");

        assert!(!src.exists(), "src should be gone after rename");
        assert_eq!(fs::read(&dst).unwrap(), b"new content");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_atomic_works_when_dst_absent() {
        let dir = temp_dir("absent-dst");
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");

        fs::write(&src, b"hello").unwrap();

        rename_atomic(&src, &dst).expect("rename_atomic must work when dst does not exist");

        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = temp_dir("write-atomic");
        let dst = dir.join("state.json");

        fs::write(&dst, b"old").unwrap();
        write_atomic(&dst, "new\n").expect("write_atomic must replace existing file");

        assert_eq!(fs::read_to_string(&dst).unwrap(), "new\n");
        assert!(
            fs::read_dir(&dir).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")),
            "temp file should be renamed away"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_jsonl_raw_creates_parent_and_syncs_line() -> std::io::Result<()> {
        let dir = temp_dir("append-jsonl");
        let path = dir.join("nested/events.jsonl");

        append_jsonl_raw(&path, r#"{"event":"one"}"#)?;
        append_jsonl_raw(&path, r#"{"event":"two"}"#)?;

        assert_eq!(
            fs::read_to_string(&path)?,
            "{\"event\":\"one\"}\n{\"event\":\"two\"}\n"
        );
        fs::remove_dir_all(&dir)
    }

    #[test]
    fn append_lines_writes_each_line_once() -> std::io::Result<()> {
        let dir = temp_dir("append-lines");
        let path = dir.join("pending.jsonl");

        append_lines(&path, &["a".to_string(), "b".to_string()])?;

        assert_eq!(fs::read_to_string(&path)?, "a\nb\n");
        fs::remove_dir_all(&dir)
    }
}
