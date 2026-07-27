use thiserror::Error;

/// Failures raised while acquiring a registry lock.
///
/// The `Display` text is part of the CLI error message contract.
#[derive(Debug, Error)]
pub enum LockError {
    #[error("LOCK_BUSY:{name}")]
    Busy { name: String },
}

/// Failure raised by the write guard that protects the Loom tool repository
/// itself from being used as a skill registry.
#[derive(Debug, Error)]
#[error(
    "refusing write operations in Loom tool repository root '{root}'; use --root <separate skill registry repo>"
)]
pub struct ToolRepoRootError {
    pub root: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_busy_message_names_the_lock() {
        let err = LockError::Busy {
            name: "workspace".to_string(),
        };
        assert_eq!(err.to_string(), "LOCK_BUSY:workspace");
    }

    #[test]
    fn tool_repo_root_message_points_at_a_separate_registry() {
        let err = ToolRepoRootError {
            root: "/repo".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "refusing write operations in Loom tool repository root '/repo'; use --root <separate skill registry repo>"
        );
    }
}
