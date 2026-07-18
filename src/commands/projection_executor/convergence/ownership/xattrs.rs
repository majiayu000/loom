#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
use std::ffi::{CString, c_char, c_void};
use std::ffi::{OsStr, OsString};
use std::io;
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
use crate::fs_util::path_to_c_string;

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
const MAX_RESIZE_ATTEMPTS: usize = 8;

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
pub(super) fn list_nofollow(path: &Path) -> io::Result<Vec<OsString>> {
    let path = path_to_c_string(path)?;
    let bytes = read_resizing(
        |buffer, length| list_call(path.as_ptr(), buffer.cast(), length),
        |_| false,
    )?
    .expect("list operation cannot report a missing attribute");
    parse_name_list(bytes)
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
pub(super) fn get_nofollow(path: &Path, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
    let path = path_to_c_string(path)?;
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "xattr name contains NUL"))?;
    read_resizing(
        |buffer, length| get_call(path.as_ptr(), name.as_ptr(), buffer, length),
        is_missing_attribute,
    )
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn read_resizing(
    mut call: impl FnMut(*mut c_void, usize) -> isize,
    missing: impl Fn(i32) -> bool,
) -> io::Result<Option<Vec<u8>>> {
    for _ in 0..MAX_RESIZE_ATTEMPTS {
        let required = call(std::ptr::null_mut(), 0);
        if required < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error().is_some_and(&missing) {
                return Ok(None);
            }
            return Err(error);
        }
        // A non-negative isize always fits in usize.
        let required = required as usize;
        if required == 0 {
            return Ok(Some(Vec::new()));
        }
        let mut bytes = vec![0u8; required];
        let written = call(bytes.as_mut_ptr().cast(), bytes.len());
        if written >= 0 {
            // A non-negative isize always fits in usize.
            bytes.truncate(written as usize);
            return Ok(Some(bytes));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error().is_some_and(&missing) {
            return Ok(None);
        }
        if error.raw_os_error() != Some(libc::ERANGE) {
            return Err(error);
        }
    }
    Err(io::Error::other("xattrs changed during read"))
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn parse_name_list(mut bytes: Vec<u8>) -> io::Result<Vec<OsString>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.pop() != Some(0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid xattr name list",
        ));
    }
    Ok(bytes
        .split(|byte| *byte == 0)
        .map(|name| OsString::from_vec(name.to_vec()))
        .collect())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn list_call(path: *const c_char, buffer: *mut c_char, length: usize) -> isize {
    // SAFETY: the caller supplies a live C path and either a null buffer with
    // zero length or writable storage of `length` bytes.
    unsafe { libc::llistxattr(path, buffer, length) }
}

#[cfg(target_os = "macos")]
fn list_call(path: *const c_char, buffer: *mut c_char, length: usize) -> isize {
    // SAFETY: the caller supplies a live C path and either a null buffer with
    // zero length or writable storage of `length` bytes.
    unsafe { libc::listxattr(path, buffer, length, libc::XATTR_NOFOLLOW) }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn get_call(path: *const c_char, name: *const c_char, buffer: *mut c_void, length: usize) -> isize {
    // SAFETY: both C strings are live and the optional output buffer is valid
    // for `length` bytes.
    unsafe { libc::lgetxattr(path, name, buffer, length) }
}

#[cfg(target_os = "macos")]
fn get_call(path: *const c_char, name: *const c_char, buffer: *mut c_void, length: usize) -> isize {
    // SAFETY: both C strings are live and the optional output buffer is valid
    // for `length` bytes.
    unsafe { libc::getxattr(path, name, buffer, length, 0, libc::XATTR_NOFOLLOW) }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn is_missing_attribute(code: i32) -> bool {
    code == libc::ENODATA
}

#[cfg(target_os = "macos")]
fn is_missing_attribute(code: i32) -> bool {
    code == libc::ENOATTR
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
pub(super) fn list_nofollow(path: &Path) -> io::Result<Vec<OsString>> {
    xattr::list(path).map(|names| names.collect())
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
pub(super) fn get_nofollow(path: &Path, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
    xattr::get(path, name)
}

#[cfg(all(
    test,
    any(target_os = "linux", target_os = "android", target_os = "macos")
))]
mod tests {
    use super::*;

    #[test]
    fn parses_kernel_name_list() {
        assert_eq!(
            parse_name_list(b"user.one\0user.two\0".to_vec()).unwrap(),
            [OsString::from("user.one"), OsString::from("user.two")]
        );
        assert!(parse_name_list(b"user.one".to_vec()).is_err());
    }
}
