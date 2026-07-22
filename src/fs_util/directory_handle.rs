use std::ffi::{CString, OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Component, Path};

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

/// An owned directory object used as the anchor for destination-side changes.
///
/// Relative operations never re-resolve the pathname used by [`Self::open`].
/// Unsupported platforms fail closed rather than falling back to path lookup.
pub(crate) struct DirectoryHandle {
    #[cfg(unix)]
    file: File,
}

impl DirectoryHandle {
    #[cfg(unix)]
    pub(crate) fn matches_path(&self, path: &Path) -> io::Result<bool> {
        use std::os::unix::fs::MetadataExt;

        let opened = self.file.metadata()?;
        let current = match std::fs::metadata(path) {
            Ok(current) => current,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(opened.dev() == current.dev() && opened.ino() == current.ino())
    }

    #[cfg(not(unix))]
    pub(crate) fn matches_path(&self, _path: &Path) -> io::Result<bool> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let mut current = if path.is_absolute() {
            Self::open_anchor(Path::new("/"))?
        } else {
            Self::open_anchor(Path::new("."))?
        };
        for component in path.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => current = current.open_child_dir(name)?,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "opened directory path contains an unsupported component",
                    ));
                }
            }
        }
        Ok(current)
    }

    #[cfg(unix)]
    fn open_anchor(path: &Path) -> io::Result<Self> {
        let path = c_string(path.as_os_str())?;
        // SAFETY: `path` is a live NUL-terminated string. The returned fd is
        // immediately transferred to `File` on success.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `open` returned a new owned descriptor.
        Ok(Self {
            file: unsafe { File::from_raw_fd(fd) },
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn open(_path: &Path) -> io::Result<Self> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn open_or_create(path: &Path) -> io::Result<Self> {
        let mut current = if path.is_absolute() {
            Self::open_anchor(Path::new("/"))?
        } else {
            Self::open_anchor(Path::new("."))?
        };
        for component in path.components() {
            let name = match component {
                Component::RootDir | Component::CurDir => continue,
                Component::Normal(name) => name,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "created directory path contains an unsupported component",
                    ));
                }
            };
            current = match current.open_child_dir(name) {
                Ok(next) => next,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    current.create_dir(Path::new(name))?;
                    current.sync()?;
                    current.open_child_dir(name)?
                }
                Err(error) => return Err(error),
            };
        }
        Ok(current)
    }

    #[cfg(not(unix))]
    pub(crate) fn open_or_create(_path: &Path) -> io::Result<Self> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn open_dir(&self, relative: &Path) -> io::Result<Self> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        parent.open_child_dir(&name)
    }

    #[cfg(not(unix))]
    pub(crate) fn open_dir(&self, _relative: &Path) -> io::Result<Self> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn create_dir(&self, relative: &Path) -> io::Result<()> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        let name = c_string(&name)?;
        // SAFETY: the parent descriptor and component string remain live.
        let result = unsafe { libc::mkdirat(parent.file.as_raw_fd(), name.as_ptr(), 0o755) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn create_dir(&self, _relative: &Path) -> io::Result<()> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn create_dir_all(&self, relative: &Path) -> io::Result<Self> {
        let mut current = self.try_clone()?;
        for component in components(relative)? {
            match current.open_child_dir(&component) {
                Ok(next) => current = next,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    current.create_dir(Path::new(&component))?;
                    current = current.open_child_dir(&component)?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(current)
    }

    #[cfg(not(unix))]
    pub(crate) fn create_dir_all(&self, _relative: &Path) -> io::Result<Self> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn entry_exists(&self, relative: &Path) -> io::Result<bool> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        let name = c_string(&name)?;
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: the input pointers are live and `metadata` has enough space.
        let result = unsafe {
            libc::fstatat(
                parent.file.as_raw_fd(),
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            return Ok(true);
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            Ok(false)
        } else {
            Err(error)
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn entry_exists(&self, _relative: &Path) -> io::Result<bool> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn write_new_synced(&self, relative: &Path, contents: &[u8]) -> io::Result<()> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        let mut file = parent.create_file(&name)?;
        file.write_all(contents)?;
        file.sync_all()
    }

    #[cfg(not(unix))]
    pub(crate) fn write_new_synced(&self, _relative: &Path, _contents: &[u8]) -> io::Result<()> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn read(&self, relative: &Path) -> io::Result<Vec<u8>> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        let mut file = parent.open_file(&name)?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        Ok(raw)
    }

    #[cfg(not(unix))]
    pub(crate) fn read(&self, _relative: &Path) -> io::Result<Vec<u8>> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn copy_file(&self, source: &Path, relative: &Path) -> io::Result<()> {
        use std::os::unix::fs::MetadataExt;

        let (parent, name) = self.resolve_parent(relative, false)?;
        let mut source = File::open(source)?;
        let mut destination = parent.create_file(&name)?;
        io::copy(&mut source, &mut destination)?;
        let mode = source.metadata()?.mode() & 0o7777;
        // SAFETY: `destination` owns a valid descriptor and mode contains only
        // Unix permission/special bits copied from the source metadata.
        if unsafe { libc::fchmod(destination.as_raw_fd(), mode as libc::mode_t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        destination.sync_all()
    }

    #[cfg(not(unix))]
    pub(crate) fn copy_file(&self, _source: &Path, _relative: &Path) -> io::Result<()> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn symlink(&self, target: &Path, relative: &Path) -> io::Result<()> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        let target = c_string(target.as_os_str())?;
        let name = c_string(&name)?;
        // SAFETY: all descriptors and C strings remain live for the call.
        let result =
            unsafe { libc::symlinkat(target.as_ptr(), parent.file.as_raw_fd(), name.as_ptr()) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(unix)]
    pub(crate) fn remove_tree(&self, relative: &Path) -> io::Result<()> {
        let (parent, name) = self.resolve_parent(relative, false)?;
        parent.remove_child_tree(&name)
    }

    #[cfg(not(unix))]
    pub(crate) fn remove_tree(&self, _relative: &Path) -> io::Result<()> {
        Err(unsupported())
    }

    #[cfg(not(unix))]
    pub(crate) fn symlink(&self, _target: &Path, _relative: &Path) -> io::Result<()> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn rename_no_replace_to(
        &self,
        source: &Path,
        destination_dir: &Self,
        destination: &Path,
    ) -> io::Result<()> {
        self.rename_to(source, destination_dir, destination, RenameMode::NoReplace)
    }

    #[cfg(not(unix))]
    pub(crate) fn rename_no_replace_to(
        &self,
        _source: &Path,
        _destination_dir: &Self,
        _destination: &Path,
    ) -> io::Result<()> {
        Err(unsupported())
    }

    #[cfg(unix)]
    pub(crate) fn exchange_to(
        &self,
        source: &Path,
        destination_dir: &Self,
        destination: &Path,
    ) -> io::Result<()> {
        self.rename_to(source, destination_dir, destination, RenameMode::Exchange)
    }

    #[cfg(not(unix))]
    pub(crate) fn exchange_to(
        &self,
        _source: &Path,
        _destination_dir: &Self,
        _destination: &Path,
    ) -> io::Result<()> {
        Err(unsupported())
    }

    pub(crate) fn sync(&self) -> io::Result<()> {
        #[cfg(unix)]
        return self.file.sync_all();
        #[cfg(not(unix))]
        Err(unsupported())
    }

    #[cfg(unix)]
    fn open_child_dir(&self, name: &OsStr) -> io::Result<Self> {
        let name = c_string(name)?;
        // SAFETY: `self` and the component string remain live. O_NOFOLLOW
        // rejects a substituted symlink rather than escaping the anchor.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a new owned descriptor.
        Ok(Self {
            file: unsafe { File::from_raw_fd(fd) },
        })
    }

    #[cfg(unix)]
    fn create_file(&self, name: &OsStr) -> io::Result<File> {
        let name = c_string(name)?;
        // SAFETY: the directory fd and component string remain live.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o644,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    #[cfg(unix)]
    fn open_file(&self, name: &OsStr) -> io::Result<File> {
        let name = c_string(name)?;
        // SAFETY: the directory fd and component string remain live.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `openat` returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    #[cfg(unix)]
    fn remove_child_tree(&self, name: &OsStr) -> io::Result<()> {
        match self.open_child_dir(name) {
            Ok(child) => {
                for entry in child.entry_names()? {
                    child.remove_child_tree(&entry)?;
                }
                let name = c_string(name)?;
                // SAFETY: the directory fd and component string remain live.
                let result = unsafe {
                    libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR)
                };
                if result == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(libc::ENOTDIR) | Some(libc::ELOOP)
                ) =>
            {
                let name = c_string(name)?;
                // SAFETY: the directory fd and component string remain live.
                let result = unsafe { libc::unlinkat(self.file.as_raw_fd(), name.as_ptr(), 0) };
                if result == 0 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(unix)]
    fn entry_names(&self) -> io::Result<Vec<OsString>> {
        // SAFETY: dup returns a new descriptor or -1 and does not affect the
        // handle owned by `self`.
        let duplicate = unsafe { libc::dup(self.file.as_raw_fd()) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fdopendir takes ownership of the duplicated descriptor.
        let directory = unsafe { libc::fdopendir(duplicate) };
        if directory.is_null() {
            // SAFETY: fdopendir failed and did not take ownership.
            unsafe { libc::close(duplicate) };
            return Err(io::Error::last_os_error());
        }
        let mut names = Vec::new();
        loop {
            // SAFETY: `directory` remains valid until closed below.
            let entry = unsafe { libc::readdir(directory) };
            if entry.is_null() {
                break;
            }
            // SAFETY: d_name is NUL-terminated for the live dirent.
            let raw = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if raw != b"." && raw != b".." {
                names.push(OsStr::from_bytes(raw).to_os_string());
            }
        }
        // SAFETY: this closes both the DIR and duplicated descriptor.
        if unsafe { libc::closedir(directory) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(names)
    }

    #[cfg(unix)]
    fn resolve_parent(&self, relative: &Path, create: bool) -> io::Result<(Self, OsString)> {
        let mut parts = components(relative)?;
        let name = parts.pop().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "relative entry is empty")
        })?;
        let mut parent = self.try_clone()?;
        for component in parts {
            parent = match parent.open_child_dir(&component) {
                Ok(next) => next,
                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                    parent.create_dir(Path::new(&component))?;
                    parent.open_child_dir(&component)?
                }
                Err(error) => return Err(error),
            };
        }
        Ok((parent, name))
    }

    #[cfg(unix)]
    fn rename_to(
        &self,
        source: &Path,
        destination_dir: &Self,
        destination: &Path,
        mode: RenameMode,
    ) -> io::Result<()> {
        let (source_parent, source_name) = self.resolve_parent(source, false)?;
        let (destination_parent, destination_name) =
            destination_dir.resolve_parent(destination, false)?;
        let source_name = c_string(&source_name)?;
        let destination_name = c_string(&destination_name)?;

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            // SAFETY: both owned directory descriptors and component strings
            // remain live for the duration of the atomic operation.
            let result = unsafe {
                const RENAME_EXCL: libc::c_uint = 0x4;
                let flags = match mode {
                    RenameMode::NoReplace => RENAME_EXCL,
                    RenameMode::Exchange => libc::RENAME_SWAP,
                };
                libc::renameatx_np(
                    source_parent.file.as_raw_fd(),
                    source_name.as_ptr(),
                    destination_parent.file.as_raw_fd(),
                    destination_name.as_ptr(),
                    flags,
                )
            };
            rename_result(result)
        }

        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            // SAFETY: both owned directory descriptors and component strings
            // remain live for the duration of the atomic operation.
            let result = unsafe {
                let flags = match mode {
                    RenameMode::NoReplace => libc::RENAME_NOREPLACE,
                    RenameMode::Exchange => libc::RENAME_EXCHANGE,
                };
                libc::renameat2(
                    source_parent.file.as_raw_fd(),
                    source_name.as_ptr(),
                    destination_parent.file.as_raw_fd(),
                    destination_name.as_ptr(),
                    flags,
                )
            };
            rename_result(result)
        }

        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android"
        )))]
        Err(unsupported())
    }
}

#[cfg(unix)]
enum RenameMode {
    NoReplace,
    Exchange,
}

#[cfg(unix)]
fn components(path: &Path) -> io::Result<Vec<OsString>> {
    let mut output = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => output.push(name.to_os_string()),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory-handle path must contain only relative normal components",
                ));
            }
        }
    }
    Ok(output)
}

#[cfg(unix)]
fn c_string(value: &OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "filesystem entry contains an interior NUL byte",
        )
    })
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn rename_result(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(super::normalize_atomic_rename_error(
            io::Error::last_os_error(),
        ))
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
)))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "handle-relative directory operations are unavailable on this platform",
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::DirectoryHandle;
    use std::fs;
    use std::path::Path;

    #[test]
    fn relative_write_stays_with_opened_directory_after_path_replacement() -> std::io::Result<()> {
        let base = std::fs::canonicalize(std::env::temp_dir())?.join(format!(
            "loom-directory-handle-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let original = base.join("target");
        let held = base.join("held");
        let outside = base.join("outside");
        fs::create_dir_all(&original)?;
        fs::create_dir_all(&outside)?;
        let handle = DirectoryHandle::open(&original)?;

        fs::rename(&original, &held)?;
        std::os::unix::fs::symlink(&outside, &original)?;
        handle.create_dir(Path::new("owner"))?;
        let owner = handle.open_dir(Path::new("owner"))?;
        owner.write_new_synced(Path::new("proof"), b"owned\n")?;

        assert_eq!(fs::read(held.join("owner/proof"))?, b"owned\n");
        assert!(!outside.join("owner").exists());
        fs::remove_file(&original)?;
        fs::remove_dir_all(base)
    }

    #[test]
    fn relative_atomic_operations_use_both_opened_directories() -> std::io::Result<()> {
        let base = std::fs::canonicalize(std::env::temp_dir())?.join(format!(
            "loom-directory-rename-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let owner_path = base.join("owner");
        let target_path = base.join("target");
        fs::create_dir_all(owner_path.join("stage"))?;
        fs::create_dir_all(target_path.join("live"))?;
        fs::write(owner_path.join("stage/new"), b"new")?;
        fs::write(target_path.join("live/old"), b"old")?;
        let owner = DirectoryHandle::open(&owner_path)?;
        let target = DirectoryHandle::open(&target_path)?;

        owner.exchange_to(Path::new("stage"), &target, Path::new("live"))?;

        assert_eq!(fs::read(target_path.join("live/new"))?, b"new");
        assert_eq!(fs::read(owner_path.join("stage/old"))?, b"old");
        fs::remove_dir_all(base)
    }

    #[test]
    fn open_rejects_a_symlinked_ancestor() -> std::io::Result<()> {
        let base = std::fs::canonicalize(std::env::temp_dir())?.join(format!(
            "loom-directory-ancestor-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let real = base.join("real");
        fs::create_dir_all(real.join("target"))?;
        std::os::unix::fs::symlink(&real, base.join("substituted"))?;

        let error = match DirectoryHandle::open(&base.join("substituted/target")) {
            Ok(_) => panic!("symlinked ancestor must fail closed"),
            Err(error) => error,
        };

        assert!(matches!(
            error.raw_os_error(),
            Some(libc::ELOOP | libc::ENOTDIR)
        ));
        fs::remove_dir_all(base)
    }
}
