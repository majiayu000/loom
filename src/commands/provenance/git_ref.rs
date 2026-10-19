use std::path::Path;

use super::{CommandFailure, ErrorCode, map_git, run_git_allow_failure_in};

pub(super) fn resolve_commit(repo: &Path, reference: &str) -> Result<String, CommandFailure> {
    // A clone only creates a local branch for its HEAD. Resolve other branch
    // names against origin before checkout, avoiding checkout's tracking DWIM.
    let candidates = [
        format!("{reference}^{{commit}}"),
        format!("refs/remotes/origin/{reference}^{{commit}}"),
        format!("refs/tags/{reference}^{{commit}}"),
    ];
    for candidate in &candidates {
        let output = run_git_allow_failure_in(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                candidate,
            ],
        )
        .map_err(map_git)?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }
    }
    Err(CommandFailure::new(
        ErrorCode::ArgInvalid,
        format!(
            "failed to resolve ref '{}' to a commit; tried: {}",
            reference,
            candidates.join(", ")
        ),
    ))
}
