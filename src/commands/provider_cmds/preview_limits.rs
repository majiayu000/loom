use std::path::Path;

use serde_json::json;
use walkdir::WalkDir;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use crate::types::ErrorCode;

const PREVIEW_MAX_ENTRIES: usize = 2_048;
const PREVIEW_MAX_DEPTH: usize = 64;
const PREVIEW_MAX_PATH_BYTES: usize = 4_096;
const PREVIEW_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const PREVIEW_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn enforce_preview_resource_limits(
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    enforce_preview_resource_limits_with_max_entries(path, PREVIEW_MAX_ENTRIES)
}

fn enforce_preview_resource_limits_with_max_entries(
    path: &Path,
    max_entries: usize,
) -> std::result::Result<(), CommandFailure> {
    let mut entries = 0_usize;
    let mut files = 0_usize;
    let mut total_bytes = 0_u64;
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(map_io)?;
        entries += 1;
        let relative_path_bytes = entry
            .path()
            .strip_prefix(path)
            .map_err(map_io)?
            .as_os_str()
            .to_string_lossy()
            .len();
        if entry.file_type().is_symlink() {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                "provider preview rejects symlink entries before local inspection",
            );
            failure.details = json!({
                "path": entry.path().strip_prefix(path).map_err(map_io)?,
                "entry_type": "symlink",
            });
            return Err(failure);
        }
        if !entry.file_type().is_file() && !entry.file_type().is_dir() {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                "provider preview rejects unsupported filesystem entries before local inspection",
            );
            failure.details = json!({
                "path": entry.path().strip_prefix(path).map_err(map_io)?,
                "entry_type": "unsupported",
            });
            return Err(failure);
        }
        let mut current_file_bytes = 0_u64;
        if entry.file_type().is_file() {
            files += 1;
            current_file_bytes = entry.metadata().map_err(map_io)?.len();
            total_bytes = total_bytes.saturating_add(current_file_bytes);
        }

        if entries > max_entries
            || entry.depth() > PREVIEW_MAX_DEPTH
            || relative_path_bytes > PREVIEW_MAX_PATH_BYTES
            || current_file_bytes > PREVIEW_MAX_FILE_BYTES
            || total_bytes > PREVIEW_MAX_TOTAL_BYTES
        {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                "provider preview exceeds the bounded local inspection budget",
            );
            failure.details = json!({
                "entries": entries,
                "files": files,
                "total_bytes": total_bytes,
                "current_file_bytes": current_file_bytes,
                "current_depth": entry.depth(),
                "current_path_bytes": relative_path_bytes,
                "limits": {
                    "entries": max_entries,
                    "depth": PREVIEW_MAX_DEPTH,
                    "path_bytes": PREVIEW_MAX_PATH_BYTES,
                    "file_bytes": PREVIEW_MAX_FILE_BYTES,
                    "total_bytes": PREVIEW_MAX_TOTAL_BYTES,
                }
            });
            return Err(failure);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn preview_rejects_a_single_oversized_file_before_reading_it() {
        let root = std::env::temp_dir().join(format!(
            "loom-provider-preview-limit-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create provider preview fixture");
        let oversized = fs::File::create(root.join("oversized.bin")).expect("create sparse file");
        oversized
            .set_len(PREVIEW_MAX_FILE_BYTES + 1)
            .expect("extend sparse file");

        let error =
            enforce_preview_resource_limits(&root).expect_err("oversized preview must fail");
        assert_eq!(error.code, ErrorCode::DependencyConflict);
        assert_eq!(
            error.details["limits"]["file_bytes"],
            PREVIEW_MAX_FILE_BYTES
        );
        fs::remove_dir_all(root).expect("remove provider preview fixture");
    }

    #[test]
    fn preview_counts_directories_toward_the_entry_budget() {
        let root = std::env::temp_dir().join(format!(
            "loom-provider-preview-entry-limit-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("first")).expect("create first preview directory");
        fs::create_dir_all(root.join("second")).expect("create second preview directory");

        let error = enforce_preview_resource_limits_with_max_entries(&root, 2)
            .expect_err("non-file entries must consume the preview budget");
        assert_eq!(error.code, ErrorCode::DependencyConflict);
        assert_eq!(error.details["limits"]["entries"], 2);
        assert_eq!(error.details["entries"], 3);
        fs::remove_dir_all(root).expect("remove provider preview fixture");
    }

    #[cfg(unix)]
    #[test]
    fn preview_rejects_symlinked_entrypoint_before_reading_oversized_target() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "loom-provider-preview-symlink-limit-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).expect("create provider preview fixture");
        let target = root.with_extension("oversized-target");
        let oversized = fs::File::create(&target).expect("create oversized symlink target");
        oversized
            .set_len(PREVIEW_MAX_FILE_BYTES + 1)
            .expect("extend oversized symlink target");
        symlink(&target, root.join("SKILL.md")).expect("create entrypoint symlink");

        let error = enforce_preview_resource_limits(&root)
            .expect_err("symlinked entrypoint must fail before lint reads its target");
        assert_eq!(error.code, ErrorCode::DependencyConflict);
        assert_eq!(error.details["entry_type"], "symlink");
        fs::remove_dir_all(root).expect("remove provider preview fixture");
        fs::remove_file(target).expect("remove oversized symlink target");
    }
}
