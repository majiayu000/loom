//! Cross-platform filesystem utilities.
//!
//! `std::fs::rename` on Windows fails with `ERROR_ALREADY_EXISTS` when the
//! destination path already exists. Every atomic-overwrite path (write-to-tmp
//! then replace) must use [`rename_atomic`] instead of `std::fs::rename`.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

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
}
