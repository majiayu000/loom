use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::Path;

use anyhow::Context;
use walkdir::WalkDir;

use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

use super::super::super::CommandFailure;
use super::super::super::provenance::skill_tree_digest;

#[cfg(unix)]
mod xattrs;

#[inline(never)]
pub(crate) fn projection_ownership_fingerprint(path: &Path) -> anyhow::Result<String> {
    let content_digest = skill_tree_digest(path)?;
    let entries = WalkDir::new(path)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.with_context(|| format!("walk {}", path.display())));

    let mut hasher = Sha256::new();
    hasher.update(b"loom-projection-ownership-v1\0");
    hasher.update(content_digest.as_bytes());
    for entry in entries {
        let entry = entry?;
        let full = entry.path();
        let relative = full
            .strip_prefix(path)
            .with_context(|| format!("strip {}", path.display()))?;
        hash_os_str(&mut hasher, relative.as_os_str());

        let metadata =
            fs::symlink_metadata(full).with_context(|| format!("stat {}", full.display()))?;
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            hasher.update(b"directory\0");
        } else if file_type.is_symlink() {
            hasher.update(b"symlink\0");
            hash_os_str(
                &mut hasher,
                fs::read_link(full)
                    .with_context(|| format!("readlink {}", full.display()))?
                    .as_os_str(),
            );
        } else if file_type.is_file() {
            hasher.update(b"file\0");
        } else {
            hasher.update(b"special\0");
        }
        hash_ownership_metadata(&mut hasher, full, &metadata, file_type.is_file())?;
        hasher.update(b"entry-end\0");
    }
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

#[inline(never)]
pub(crate) fn map_ownership_fingerprint_error(error: anyhow::Error, path: &Path) -> CommandFailure {
    let code = if error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(io_error_is_unsupported)
    }) {
        ErrorCode::ProjectionMethodUnsupported
    } else {
        ErrorCode::ProjectionConflict
    };
    CommandFailure::new(
        code,
        format!(
            "projection fingerprint failed for '{}': {error:#}",
            path.display()
        ),
    )
}

fn io_error_is_unsupported(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(unix)]
    if error.raw_os_error().is_some_and(|code| {
        code == libc::ENOTSUP || code == libc::EOPNOTSUPP || code == libc::ENOSYS
    }) {
        return true;
    }
    #[cfg(windows)]
    if error.raw_os_error().is_some_and(|code| {
        use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED};

        code == ERROR_INVALID_PARAMETER as i32 || code == ERROR_NOT_SUPPORTED as i32
    }) {
        return true;
    }
    false
}

#[cfg(unix)]
fn hash_os_str(hasher: &mut Sha256, value: &OsStr) {
    use std::os::unix::ffi::OsStrExt;

    let bytes = value.as_bytes();
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(windows)]
fn hash_os_str(hasher: &mut Sha256, value: &OsStr) {
    use std::os::windows::ffi::OsStrExt;

    let words = value.encode_wide().collect::<Vec<_>>();
    hasher.update(&(words.len() as u64).to_be_bytes());
    for word in words {
        hasher.update(&word.to_be_bytes());
    }
}

#[cfg(unix)]
fn hash_ownership_metadata(
    hasher: &mut Sha256,
    path: &Path,
    metadata: &fs::Metadata,
    include_write_time: bool,
) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    for value in [
        metadata.dev(),
        metadata.ino(),
        u64::from(metadata.mode()),
        metadata.nlink(),
        u64::from(metadata.uid()),
        u64::from(metadata.gid()),
        metadata.rdev(),
    ] {
        hasher.update(&value.to_be_bytes());
    }
    if include_write_time {
        hasher.update(&metadata.size().to_be_bytes());
        hasher.update(&metadata.mtime().to_be_bytes());
        hasher.update(&metadata.mtime_nsec().to_be_bytes());
    }
    hash_xattrs(hasher, path)?;
    #[cfg(target_os = "macos")]
    hash_macos_acl(hasher, path)?;
    Ok(())
}

#[cfg(unix)]
fn hash_xattrs(hasher: &mut Sha256, path: &Path) -> anyhow::Result<()> {
    let mut names =
        xattrs::list_nofollow(path).with_context(|| format!("list xattrs {}", path.display()))?;
    names.sort();
    for name in names {
        hasher.update(b"xattr\0");
        hash_os_str(hasher, &name);
        let value = xattrs::get_nofollow(path, &name)
            .with_context(|| format!("read xattr {:?} on {}", name, path.display()))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "xattr {:?} disappeared while fingerprinting {}",
                    name,
                    path.display()
                )
            })?;
        hasher.update(&(value.len() as u64).to_be_bytes());
        hasher.update(&value);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn hash_macos_acl(hasher: &mut Sha256, path: &Path) -> anyhow::Result<()> {
    use std::ffi::{CString, c_char, c_int, c_void};
    use std::os::unix::ffi::OsStrExt;

    const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
    unsafe extern "C" {
        fn acl_get_link_np(path: *const c_char, acl_type: c_int) -> *mut c_void;
        fn acl_to_text(acl: *mut c_void, len: *mut isize) -> *mut c_char;
        fn acl_free(object: *mut c_void) -> c_int;
    }

    let path_bytes = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path contains an interior NUL byte: {}", path.display()),
        )
    })?;
    // SAFETY: the C path remains live and NUL-terminated for the call.
    let acl = unsafe { acl_get_link_np(path_bytes.as_ptr(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ENOENT) && fs::symlink_metadata(path).is_ok() {
            hasher.update(b"macos-acl\0");
            hasher.update(&0u64.to_be_bytes());
            return Ok(());
        }
        let error = if io_error_is_unsupported(&error) {
            io::Error::new(io::ErrorKind::Unsupported, error)
        } else {
            error
        };
        return Err(error).with_context(|| format!("read ACL {}", path.display()));
    }
    struct AclAllocation(*mut c_void);
    impl Drop for AclAllocation {
        fn drop(&mut self) {
            // SAFETY: the allocation came from an ACL libc function and is
            // released exactly once by this guard.
            unsafe { acl_free(self.0) };
        }
    }
    let _acl_guard = AclAllocation(acl);
    let mut length = 0isize;
    // SAFETY: `acl` is live, and `length` is valid writable storage.
    let text = unsafe { acl_to_text(acl, &raw mut length) };
    if text.is_null() {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("serialize ACL {}", path.display()));
    }
    let _text_guard = AclAllocation(text.cast());
    let length = usize::try_from(length).context("ACL text length was negative")?;
    // SAFETY: acl_to_text returned a buffer containing at least `length`
    // initialized bytes, and the allocation remains live through the hash.
    let bytes = unsafe { std::slice::from_raw_parts(text.cast::<u8>(), length) };
    hasher.update(b"macos-acl\0");
    hasher.update(&(length as u64).to_be_bytes());
    hasher.update(bytes);
    Ok(())
}

#[cfg(windows)]
fn hash_ownership_metadata(
    hasher: &mut Sha256,
    path: &Path,
    metadata: &fs::Metadata,
    include_write_time: bool,
) -> anyhow::Result<()> {
    use std::os::windows::fs::MetadataExt;

    for value in [
        u64::from(metadata.file_attributes()),
        metadata.creation_time(),
    ] {
        hasher.update(&value.to_be_bytes());
    }
    let (volume_serial, file_id, security_descriptor) = windows_file_ownership(path)?;
    hasher.update(&volume_serial.to_be_bytes());
    hasher.update(&file_id);
    hasher.update(b"windows-security-descriptor\0");
    hasher.update(&(security_descriptor.len() as u64).to_be_bytes());
    hasher.update(&security_descriptor);
    if include_write_time {
        hasher.update(&metadata.file_size().to_be_bytes());
        hasher.update(&metadata.last_write_time().to_be_bytes());
    }
    Ok(())
}

#[cfg(windows)]
fn windows_file_ownership(path: &Path) -> anyhow::Result<(u64, [u8; 16], Vec<u8>)> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO, FileIdInfo,
        GetFileInformationByHandleEx, READ_CONTROL,
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorLength,
            OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
    };

    let file = OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .with_context(|| format!("open identity handle {}", path.display()))?;
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: the handle stays open for the call, and the output pointer and
    // byte length describe a live `FILE_ID_INFO` value.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("read file identity {}", path.display()));
    }
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let security_information =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    // SAFETY: the file handle is live, optional component pointers are null,
    // and `descriptor` receives one LocalAlloc-owned security descriptor.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            security_information,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32))
            .with_context(|| format!("read security descriptor {}", path.display()));
    }
    if descriptor.is_null() {
        return Err(io::Error::other("security descriptor was null"))
            .with_context(|| format!("read security descriptor {}", path.display()));
    }
    struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            // SAFETY: GetSecurityInfo returned this allocation exactly once.
            unsafe { LocalFree(self.0) };
        }
    }
    let descriptor = SecurityDescriptor(descriptor);
    // SAFETY: the descriptor allocation remains live through the copy.
    let length = unsafe { GetSecurityDescriptorLength(descriptor.0) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("measure security descriptor {}", path.display()));
    }
    // SAFETY: GetSecurityDescriptorLength reports the initialized size of the
    // self-relative descriptor owned by `descriptor`.
    let bytes = unsafe { std::slice::from_raw_parts(descriptor.0.cast::<u8>(), length) }.to_vec();
    Ok((
        identity.VolumeSerialNumber,
        identity.FileId.Identifier,
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn fingerprint_is_stable_across_atomic_directory_exchange() {
        for attempt in 0..256 {
            let root = std::env::temp_dir().join(format!(
                "loom-fingerprint-exchange-{}-{attempt}",
                uuid::Uuid::new_v4().simple()
            ));
            let live = root.join("live");
            let staged = root.join("owner/stage");
            fs::create_dir_all(&live).expect("create live");
            fs::create_dir_all(&staged).expect("create staged");
            fs::write(live.join("SKILL.md"), "old\n").expect("write live");
            fs::write(staged.join("SKILL.md"), "new\n").expect("write staged");
            fs::write(staged.join("details.txt"), "details\n").expect("write staged details");

            let expected = projection_ownership_fingerprint(&staged).expect("fingerprint staged");
            crate::fs_util::exchange_paths_atomic(&staged, &live).expect("exchange");
            let actual = projection_ownership_fingerprint(&live).expect("fingerprint live");
            assert_eq!(actual, expected, "attempt {attempt}");
            fs::remove_dir_all(root).expect("remove fixture");
        }
    }

    #[test]
    fn unsupported_metadata_backend_is_typed_method_unsupported() {
        let error = anyhow::Error::new(io::Error::new(
            io::ErrorKind::Unsupported,
            "xattrs unavailable",
        ))
        .context("list projection xattrs");

        let failure = map_ownership_fingerprint_error(error, Path::new("projection"));

        assert_eq!(failure.code, ErrorCode::ProjectionMethodUnsupported);
        assert!(failure.message.contains("xattrs unavailable"));
    }

    #[cfg(windows)]
    #[test]
    fn unsupported_windows_file_identity_codes_are_typed_method_unsupported() {
        use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED};

        for code in [ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED] {
            let error = anyhow::Error::new(io::Error::from_raw_os_error(code as i32))
                .context("read file identity");
            let failure = map_ownership_fingerprint_error(error, Path::new("projection"));
            assert_eq!(failure.code, ErrorCode::ProjectionMethodUnsupported);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_ownership_reads_a_security_descriptor() {
        let root = std::env::temp_dir().join(format!(
            "loom-windows-ownership-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir(&root).expect("create ownership fixture");

        let (_, _, descriptor) =
            windows_file_ownership(&root).expect("read Windows ownership metadata");

        assert!(!descriptor.is_empty());
        projection_ownership_fingerprint(&root)
            .expect("fingerprint with owner, group, and DACL access");
        std::fs::remove_dir(root).expect("remove ownership fixture");
    }
}
