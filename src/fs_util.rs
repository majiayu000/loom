//! Cross-platform filesystem utilities.
//!
//! `std::fs::rename` on Windows fails with `ERROR_ALREADY_EXISTS` when the
//! destination path already exists. Every atomic-overwrite path (write-to-tmp
//! then replace) must use [`rename_atomic`] instead of `std::fs::rename`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
use std::ffi::CString;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
use std::os::unix::ffi::OsStrExt;

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

/// Atomically rename `src` to `dst` only when `dst` does not exist.
///
/// This closes the gap between preparing a new entry and activating it: a
/// concurrently-created destination is preserved and reported as
/// [`io::ErrorKind::AlreadyExists`]. Unsupported platforms fail closed.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
pub fn rename_no_replace_atomic(src: &Path, dst: &Path) -> io::Result<()> {
    let src = path_to_c_string(src)?;
    let dst = path_to_c_string(dst)?;

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        // `RENAME_EXCL` is 0x4 in the Darwin SDK. libc does not currently
        // expose the constant, so keep the SDK value local to this call.
        const RENAME_EXCL: libc::c_uint = 0x4;
        // SAFETY: both C strings are NUL-terminated and remain alive for the
        // duration of the call. `RENAME_EXCL` makes existence checking part
        // of the same filesystem operation.
        let result = unsafe { libc::renamex_np(src.as_ptr(), dst.as_ptr(), RENAME_EXCL) };
        if result == 0 {
            Ok(())
        } else {
            Err(atomic_rename_os_error())
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: both C strings are valid for the call and AT_FDCWD anchors
        // their paths exactly as rename(2) does. `RENAME_NOREPLACE` performs
        // the destination existence check atomically.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                src.as_ptr(),
                libc::AT_FDCWD,
                dst.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(atomic_rename_os_error())
        }
    }
}

#[cfg(windows)]
pub fn rename_no_replace_atomic(src: &Path, dst: &Path) -> io::Result<()> {
    move_file_windows(src, dst, false)
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android",
    windows
)))]
pub fn rename_no_replace_atomic(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename is unavailable on this platform",
    ))
}

/// Atomically exchange two existing directory entries on the same filesystem.
///
/// Callers use the entry left at `src` as their rollback artifact. Platforms or
/// filesystems without a native exchange primitive fail closed instead of
/// emulating the swap with a remove/rename sequence.
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
pub fn exchange_paths_atomic(src: &Path, dst: &Path) -> io::Result<()> {
    let src = path_to_c_string(src)?;
    let dst = path_to_c_string(dst)?;

    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        // SAFETY: both C strings are NUL-terminated and remain alive for the
        // duration of the call. `RENAME_SWAP` performs one filesystem op.
        let result = unsafe { libc::renamex_np(src.as_ptr(), dst.as_ptr(), libc::RENAME_SWAP) };
        if result == 0 {
            Ok(())
        } else {
            Err(atomic_rename_os_error())
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: both C strings are valid for the call and AT_FDCWD anchors
        // their absolute or process-relative paths exactly as rename(2) does.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                src.as_ptr(),
                libc::AT_FDCWD,
                dst.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(atomic_rename_os_error())
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn atomic_rename_os_error() -> io::Error {
    normalize_atomic_rename_error(io::Error::last_os_error())
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn normalize_atomic_rename_error(err: io::Error) -> io::Error {
    let unsupported = match err.raw_os_error() {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        Some(code) if code == libc::EINVAL || code == libc::ENOSYS || code == libc::EOPNOTSUPP => {
            true
        }
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        Some(code) if code == libc::EINVAL || code == libc::ENOTSUP => true,
        _ => false,
    };
    if unsupported {
        io::Error::new(io::ErrorKind::Unsupported, err)
    } else {
        err
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
pub(crate) fn path_to_c_string(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains an interior NUL byte: {}", path.display()),
        )
    })
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
)))]
pub fn exchange_paths_atomic(_src: &Path, _dst: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic path exchange is unavailable on this platform",
    ))
}

/// Write UTF-8 contents through a temp file and atomically replace the destination.
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    write_atomic_bytes(path, contents.as_bytes())
}

/// Write bytes through a temp file and atomically replace the destination.
pub fn write_atomic_bytes(path: &Path, contents: &[u8]) -> io::Result<()> {
    maybe_fault_inject("write_atomic")?;
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
        file.write_all(contents)?;
        file.sync_all()?;
    }

    rename_atomic(&tmp_path, path)?;
    sync_parent_directory(path)
}

/// Persist the directory entry containing `path` after a create or rename.
///
/// File synchronization alone does not make a newly published name durable.
/// Unsupported platforms fail closed instead of claiming crash durability.
#[cfg(unix)]
pub fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    File::open(parent)?.sync_all()
}

#[cfg(windows)]
pub fn sync_parent_directory(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(parent)?
        .sync_all()
}

#[cfg(not(any(unix, windows)))]
pub fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "durable parent directory synchronization is unavailable",
    ))
}

/// Return whether both existing paths resolve to entries on the same filesystem.
///
/// Windows opens the resolved target (without `FILE_FLAG_OPEN_REPARSE_POINT`)
/// so a junction or symlink cannot make a cross-volume activation appear local.
#[cfg(unix)]
pub(crate) fn paths_share_filesystem(left: &Path, right: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    Ok(fs::metadata(left)?.dev() == fs::metadata(right)?.dev())
}

#[cfg(windows)]
pub(crate) fn paths_share_filesystem(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(windows_volume_serial(left)? == windows_volume_serial(right)?)
}

#[cfg(windows)]
fn windows_volume_serial(path: &Path) -> io::Result<u64> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let handle = OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: `handle` remains open for the call, while the output pointer and
    // length describe a live, correctly aligned `FILE_ID_INFO` value.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(identity.VolumeSerialNumber)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn paths_share_filesystem(_left: &Path, _right: &Path) -> io::Result<bool> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "filesystem identity comparison is unavailable on this platform",
    ))
}

/// Append newline-terminated records and sync the file.
#[allow(dead_code)]
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

#[cfg(unix)]
pub fn remove_symlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
pub fn remove_symlink(path: &Path) -> io::Result<()> {
    fs::remove_dir(path)
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
    move_file_windows(src, dst, true)
}

#[cfg(windows)]
fn move_file_windows(src: &Path, dst: &Path, replace_existing: bool) -> io::Result<()> {
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
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace_existing {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    let ok = unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), flags) };
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

    #[cfg(any(unix, windows))]
    #[test]
    fn filesystem_comparison_accepts_entries_on_the_same_volume() -> io::Result<()> {
        let dir = temp_dir("same-filesystem");
        let child = dir.join("child");
        fs::create_dir(&child)?;

        assert!(paths_share_filesystem(&dir, &child)?);
        fs::remove_dir_all(&dir)
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        windows
    ))]
    #[test]
    fn rename_no_replace_atomic_works_when_dst_absent() {
        let dir = temp_dir("no-replace-absent");
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");
        fs::write(&src, b"new content").unwrap();

        rename_no_replace_atomic(&src, &dst).expect("absent destination must be activated");

        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"new content");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        windows
    ))]
    #[test]
    fn rename_no_replace_atomic_preserves_existing_destination() {
        let dir = temp_dir("no-replace-existing");
        let src = dir.join("src.txt");
        let dst = dir.join("dst.txt");
        fs::write(&src, b"staged content").unwrap();
        fs::write(&dst, b"concurrent content").unwrap();

        let error = rename_no_replace_atomic(&src, &dst)
            .expect_err("existing destination must fail closed");

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&src).unwrap(), b"staged content");
        assert_eq!(fs::read(&dst).unwrap(), b"concurrent content");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    ))]
    #[test]
    fn atomic_rename_invalid_argument_is_typed_unsupported() {
        let error = normalize_atomic_rename_error(io::Error::from_raw_os_error(libc::EINVAL));
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android",
        windows
    )))]
    #[test]
    fn rename_no_replace_atomic_fails_closed_when_unsupported() {
        let error = rename_no_replace_atomic(Path::new("src"), Path::new("dst"))
            .expect_err("unsupported platform must fail closed");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    ))]
    #[test]
    fn exchange_paths_atomic_swaps_nonempty_directories_without_missing_entry() -> io::Result<()> {
        let dir = temp_dir("exchange-directories");
        let src = dir.join("staged");
        let dst = dir.join("live");
        fs::create_dir_all(&src)?;
        fs::create_dir_all(&dst)?;
        fs::write(src.join("new.txt"), "new")?;
        fs::write(dst.join("old.txt"), "old")?;

        exchange_paths_atomic(&src, &dst)?;

        assert_eq!(fs::read_to_string(dst.join("new.txt"))?, "new");
        assert_eq!(fs::read_to_string(src.join("old.txt"))?, "old");
        assert!(src.is_dir() && dst.is_dir());
        fs::remove_dir_all(dir)
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
