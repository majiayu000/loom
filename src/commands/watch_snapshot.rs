use std::fs::{self, File, Metadata, Permissions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::gitops;
use crate::sha256::Sha256;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::super::CommandFailure;
use super::super::WatchPlan;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum WatchPathSnapshot {
    Missing,
    File {
        stamp: FileStamp,
        digest: [u8; 32],
    },
    Symlink {
        stamp: FileStamp,
        target: PathBuf,
    },
    Directory {
        stamp: FileStamp,
        gitlink_head: Option<String>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct FileStamp {
    length: u64,
    modified: Option<SystemTime>,
    permissions: Permissions,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl FileStamp {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            permissions: metadata.permissions(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        }
    }
}

pub(super) fn snapshot_paths(
    ctx: &AppContext,
    plan: &WatchPlan,
) -> Result<Vec<WatchPathSnapshot>, CommandFailure> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }
    let reader = SnapshotReader::new(&ctx.root).map_err(snapshot_error)?;
    plan.skills
        .iter()
        .flat_map(|skill| &skill.paths)
        .map(|path| reader.snapshot(Path::new(path)).map_err(snapshot_error))
        .collect()
}

fn snapshot_error(error: io::Error) -> CommandFailure {
    CommandFailure::new(
        ErrorCode::CaptureConflict,
        format!("could not snapshot skill files for autosave; retry after edits settle: {error}"),
    )
}

fn changed() -> io::Error {
    io::Error::other("skill file changed while taking an autosave snapshot")
}

struct SnapshotReader {
    root: PathBuf,
    #[cfg(unix)]
    directory: crate::fs_util::DirectoryHandle,
}

impl SnapshotReader {
    fn new(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root)?;
        Ok(Self {
            #[cfg(unix)]
            directory: crate::fs_util::DirectoryHandle::open(&root)?,
            root,
        })
    }

    fn snapshot(&self, relative: &Path) -> io::Result<WatchPathSnapshot> {
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(changed());
        }
        let path = self.root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(WatchPathSnapshot::Missing);
            }
            Err(error) => return Err(error),
        };
        let stamp = FileStamp::from_metadata(&metadata);
        let snapshot = if metadata.is_symlink() {
            WatchPathSnapshot::Symlink {
                target: self.read_link(relative)?,
                stamp,
            }
        } else if metadata.is_file() {
            let mut file = self.open_file(relative)?;
            if FileStamp::from_metadata(&file.metadata()?) != stamp {
                return Err(changed());
            }
            // Keep memory fixed and stop at the sampled length, even if an
            // editor keeps appending. A short read or changed stamp is a race.
            let mut remaining = stamp.length;
            let mut buffer = [0_u8; 8192];
            let mut hasher = Sha256::new();
            while remaining > 0 {
                let limit = remaining.min(buffer.len() as u64) as usize;
                let read = file.read(&mut buffer[..limit])?;
                if read == 0 {
                    return Err(changed());
                }
                hasher.update(&buffer[..read]);
                remaining -= read as u64;
            }
            if FileStamp::from_metadata(&file.metadata()?) != stamp {
                return Err(changed());
            }
            WatchPathSnapshot::File {
                stamp,
                digest: hasher.finalize(),
            }
        } else if metadata.is_dir() {
            #[cfg(unix)]
            let directory = self.directory.open_dir(relative)?;
            #[cfg(windows)]
            let _ancestors = self.hold_ancestors(&relative.join(".git"))?;
            let identity = gitops::run_git_in_dir(
                &path,
                gitops::FileProtocol::Blocked,
                &["rev-parse", "--show-prefix", "--verify", "HEAD"],
            )
            .map_err(io::Error::other)?;
            #[cfg(unix)]
            if !directory.matches_path(&path)? {
                return Err(changed());
            }
            // An empty prefix identifies a nested repository root. Ordinary
            // directories also print their prefix, not just the parent HEAD.
            // Track the gitlink commit without reading submodule contents.
            let gitlink_head = (!identity.contains('\n')).then_some(identity);
            WatchPathSnapshot::Directory {
                stamp,
                gitlink_head,
            }
        } else {
            // Never open pipes/devices, which could block or read indefinitely.
            return Err(io::Error::other(
                "autosave source is not a regular file or symlink",
            ));
        };
        if FileStamp::from_metadata(&fs::symlink_metadata(&path)?)
            != FileStamp::from_metadata(&metadata)
        {
            return Err(changed());
        }
        Ok(snapshot)
    }

    #[cfg(unix)]
    fn open_file(&self, relative: &Path) -> io::Result<File> {
        self.directory.open_regular_file(relative)
    }

    #[cfg(unix)]
    fn read_link(&self, relative: &Path) -> io::Result<PathBuf> {
        self.directory.read_link(relative)
    }

    #[cfg(windows)]
    fn open_file(&self, relative: &Path) -> io::Result<File> {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        let _ancestors = self.hold_ancestors(relative)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(self.root.join(relative))?;
        if !file.metadata()?.is_file() || !opened_path(&file)?.starts_with(&self.root) {
            return Err(changed());
        }
        Ok(file)
    }

    #[cfg(windows)]
    fn read_link(&self, relative: &Path) -> io::Result<PathBuf> {
        let _ancestors = self.hold_ancestors(relative)?;
        fs::read_link(self.root.join(relative))
    }

    #[cfg(windows)]
    fn hold_ancestors(&self, relative: &Path) -> io::Result<Vec<File>> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };

        let mut handles = Vec::new();
        let mut path = self.root.clone();
        let ancestors = std::iter::once(None).chain(
            relative
                .parent()
                .into_iter()
                .flat_map(Path::components)
                .map(Some),
        );
        for component in ancestors {
            if let Some(component) = component {
                path.push(component);
            }
            // Denying delete sharing pins every checked ancestor until the
            // leaf has been opened/read, so it cannot be swapped for a link.
            let handle = fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)?;
            let metadata = handle.metadata()?;
            if !metadata.is_dir()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                || opened_path(&handle)? != path
            {
                return Err(changed());
            }
            handles.push(handle);
        }
        Ok(handles)
    }

    #[cfg(not(any(unix, windows)))]
    fn read_link(&self, _relative: &Path) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe autosave snapshots are unsupported",
        ))
    }

    #[cfg(not(any(unix, windows)))]
    fn open_file(&self, _relative: &Path) -> io::Result<File> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe autosave snapshots are unsupported",
        ))
    }
}

#[cfg(windows)]
fn opened_path(file: &File) -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;

    let mut buffer = vec![0_u16; 32768];
    // SAFETY: the owned file handle and writable UTF-16 buffer are valid.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle() as _,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    };
    if length == 0 || length as usize >= buffer.len() {
        return Err(changed());
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}
