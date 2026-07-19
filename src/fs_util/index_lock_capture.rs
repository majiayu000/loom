use std::io;
use std::path::Path;

/// Exchange the public lock with a pre-created placeholder at `capture`.
///
/// The single filesystem operation leaves the public path occupied and the
/// previous public entry at `capture`, including if the process crashes as the
/// exchange completes.
#[cfg(unix)]
pub fn capture_with_placeholder_atomic(path: &Path, capture: &Path) -> io::Result<()> {
    super::exchange_paths_atomic(capture, path)
}

/// Restore the captured entry with one exchange. The placeholder is left at
/// `capture` for the caller to remove after the public entry is durable.
#[cfg(unix)]
pub fn restore_capture_atomic(path: &Path, capture: &Path) -> io::Result<()> {
    super::exchange_paths_atomic(capture, path)
}

#[cfg(not(unix))]
pub fn capture_with_placeholder_atomic(_path: &Path, _capture: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic placeholder capture is unavailable on this platform",
    ))
}

#[cfg(not(unix))]
pub fn restore_capture_atomic(_path: &Path, _capture: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic placeholder restoration is unavailable on this platform",
    ))
}

#[cfg(unix)]
pub fn same_file_identity_paths(left: &Path, right: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let left = std::fs::symlink_metadata(left)?;
    let right = std::fs::symlink_metadata(right)?;
    Ok(left.file_type().is_file()
        && right.file_type().is_file()
        && left.dev() == right.dev()
        && left.ino() == right.ino())
}

#[cfg(windows)]
pub fn same_file_identity_paths(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(windows_file_identity(left)? == windows_file_identity(right)?)
}

#[cfg(not(any(unix, windows)))]
pub fn same_file_identity_paths(_left: &Path, _right: &Path) -> io::Result<bool> {
    Ok(false)
}

#[cfg(windows)]
#[derive(Clone, Copy, Eq, PartialEq)]
struct WindowsFileIdentity {
    volume: u32,
    index_high: u32,
    index_low: u32,
}

#[cfg(windows)]
fn identity_from_file(file: &std::fs::File) -> io::Result<WindowsFileIdentity> {
    use std::mem::MaybeUninit;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `file` owns a valid handle and `information` is writable for the
    // exact structure required by GetFileInformationByHandle.
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, information.as_mut_ptr()) };
    if result == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful API call initialized the complete structure.
    let information = unsafe { information.assume_init() };
    Ok(WindowsFileIdentity {
        volume: information.dwVolumeSerialNumber,
        index_high: information.nFileIndexHigh,
        index_low: information.nFileIndexLow,
    })
}

#[cfg(windows)]
fn windows_file_identity(path: &Path) -> io::Result<WindowsFileIdentity> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let file = OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    identity_from_file(&file)
}

/// An exact public lock file held without sharing. While this guard exists,
/// another process cannot open, replace, rename, or delete that lock entry.
#[cfg(windows)]
pub struct ExclusiveDeleteFile {
    file: std::fs::File,
}

#[cfg(windows)]
impl ExclusiveDeleteFile {
    pub fn open_owned(path: &Path, claim: &Path, expected: &[u8]) -> io::Result<Self> {
        use std::fs::OpenOptions;
        use std::io::Read;
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        };

        let claim_identity = windows_file_identity(claim)?;
        let mut file = OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | DELETE)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        if identity_from_file(&file)? != claim_identity {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Git index lock is not owned by the durable transaction claim",
            ));
        }
        let mut actual = Vec::new();
        file.read_to_end(&mut actual)?;
        if actual != expected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "durable Git index lock changed before exclusive publication",
            ));
        }
        Ok(Self { file })
    }

    pub fn delete(self) -> io::Result<()> {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
        };

        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: the guard owns a valid exclusive handle and `disposition`
        // has the exact type and size required by FileDispositionInfo.
        let result = unsafe {
            SetFileInformationByHandle(
                self.file.as_raw_handle() as _,
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        drop(self);
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn capture_and_restore_each_use_one_exchange() {
        let root =
            std::env::temp_dir().join(format!("loom-index-lock-capture-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).expect("create test directory");
        let path = root.join("index.lock");
        let capture = root.join("capture");
        fs::write(&path, b"foreign").expect("write public entry");
        fs::write(&capture, b"placeholder").expect("write capture placeholder");

        capture_with_placeholder_atomic(&path, &capture).expect("capture entry");
        assert_eq!(fs::read(&path).expect("public placeholder"), b"placeholder");
        assert_eq!(fs::read(&capture).expect("captured entry"), b"foreign");
        restore_capture_atomic(&path, &capture).expect("restore entry");
        assert_eq!(fs::read(&path).expect("restored public entry"), b"foreign");
        assert_eq!(
            fs::read(&capture).expect("restored placeholder"),
            b"placeholder"
        );
        fs::remove_dir_all(root).expect("remove test directory");
    }
}
