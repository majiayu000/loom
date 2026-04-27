//! Cross-platform filesystem utilities.
//!
//! `std::fs::rename` on Windows fails with `ERROR_ALREADY_EXISTS` when the
//! destination path already exists. Every atomic-overwrite path (write-to-tmp
//! then replace) must use [`rename_atomic`] instead of `std::fs::rename`.

use std::path::Path;

/// Atomically replace `dst` with `src`.
///
/// On Unix this is a single `rename(2)` syscall, which is atomic at the
/// filesystem level. On Windows, where a plain rename fails when the
/// destination exists, we remove the destination first and then rename.
/// This is not a kernel-level atomic swap on Windows, but gives the same
/// practical guarantee for single-writer scenarios (no partial reads of a
/// half-written file are possible).
pub fn rename_atomic(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if dst.is_dir() {
            std::fs::remove_dir_all(dst)?;
        } else if dst.exists() {
            std::fs::remove_file(dst)?;
        }
    }
    std::fs::rename(src, dst)
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
}
