use std::path::Path;

use serde_json::json;
use walkdir::WalkDir;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use crate::types::ErrorCode;

const PREVIEW_MAX_FILES: usize = 2_048;
const PREVIEW_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const PREVIEW_MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn enforce_preview_resource_limits(
    path: &Path,
) -> std::result::Result<(), CommandFailure> {
    let mut files = 0_usize;
    let mut total_bytes = 0_u64;
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(map_io)?;
        if !entry.file_type().is_file() {
            continue;
        }
        files += 1;
        let bytes = entry.metadata().map_err(map_io)?.len();
        total_bytes = total_bytes.saturating_add(bytes);
        if files > PREVIEW_MAX_FILES
            || bytes > PREVIEW_MAX_FILE_BYTES
            || total_bytes > PREVIEW_MAX_TOTAL_BYTES
        {
            let mut failure = CommandFailure::new(
                ErrorCode::DependencyConflict,
                "provider preview exceeds the bounded local inspection budget",
            );
            failure.details = json!({
                "files": files,
                "total_bytes": total_bytes,
                "current_file_bytes": bytes,
                "limits": {
                    "files": PREVIEW_MAX_FILES,
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
}
