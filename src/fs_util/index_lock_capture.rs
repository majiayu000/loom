use std::io;
use std::path::Path;

/// Atomically replace `path` with `placeholder` while preserving the previous
/// entry at `capture`. The public path never becomes absent.
#[cfg(unix)]
pub fn capture_with_placeholder_atomic(
    path: &Path,
    placeholder: &Path,
    capture: &Path,
) -> io::Result<()> {
    super::exchange_paths_atomic(placeholder, path)?;
    if let Err(error) = super::rename_no_replace_atomic(placeholder, capture) {
        return match super::exchange_paths_atomic(placeholder, path) {
            Ok(()) => Err(error),
            Err(rollback) => Err(io::Error::new(
                error.kind(),
                format!("{error}; additionally failed to restore exchanged path: {rollback}"),
            )),
        };
    }
    Ok(())
}

/// Restore `capture` to `path` while preserving the public placeholder until
/// the captured entry is active again.
#[cfg(unix)]
pub fn restore_capture_atomic(path: &Path, capture: &Path, placeholder: &Path) -> io::Result<()> {
    super::rename_no_replace_atomic(capture, placeholder)?;
    super::exchange_paths_atomic(placeholder, path)
}

#[cfg(windows)]
pub fn capture_with_placeholder_atomic(
    path: &Path,
    placeholder: &Path,
    capture: &Path,
) -> io::Result<()> {
    replace_with_backup(path, placeholder, capture)
}

#[cfg(windows)]
pub fn restore_capture_atomic(path: &Path, capture: &Path, placeholder: &Path) -> io::Result<()> {
    replace_with_backup(path, capture, placeholder)
}

#[cfg(windows)]
fn replace_with_backup(path: &Path, replacement: &Path, backup: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

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
    // SAFETY: all paths are NUL-terminated UTF-16 buffers valid for the call;
    // the optional merge-exclusion pointers are null as required.
    let result = unsafe {
        ReplaceFileW(
            path.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            ptr::null(),
            ptr::null(),
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
pub fn capture_with_placeholder_atomic(
    _path: &Path,
    _placeholder: &Path,
    _capture: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic placeholder capture is unavailable on this platform",
    ))
}

#[cfg(not(any(unix, windows)))]
pub fn restore_capture_atomic(
    _path: &Path,
    _capture: &Path,
    _placeholder: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic placeholder restoration is unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn capture_and_restore_never_remove_the_public_entry() {
        let root =
            std::env::temp_dir().join(format!("loom-index-lock-capture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("create test directory");
        let path = root.join("index.lock");
        let placeholder = root.join("placeholder");
        let capture = root.join("capture");
        fs::write(&path, b"foreign").expect("write public entry");
        fs::write(&placeholder, b"placeholder").expect("write placeholder");

        capture_with_placeholder_atomic(&path, &placeholder, &capture).expect("capture entry");
        assert_eq!(fs::read(&path).expect("public placeholder"), b"placeholder");
        assert_eq!(fs::read(&capture).expect("captured entry"), b"foreign");
        restore_capture_atomic(&path, &capture, &placeholder).expect("restore entry");
        assert_eq!(fs::read(&path).expect("restored public entry"), b"foreign");
        assert_eq!(
            fs::read(&placeholder).expect("restored placeholder"),
            b"placeholder"
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }
}
