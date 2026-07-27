use thiserror::Error;

use crate::types::ErrorCode;

/// Registry state failures that carry their own [`ErrorCode`] classification.
///
/// Errors of this type travel inside `anyhow::Error` values returned by the
/// persistence layer. Consumers classify them with [`RegistryStateError::classify`]
/// instead of matching on message text, so renaming a message can never
/// silently change the reported error code.
#[derive(Debug, Error)]
pub enum RegistryStateError {
    #[error("registry schema version mismatch: expected {expected}, got {actual}")]
    SchemaMismatch { expected: u32, actual: u32 },
}

impl RegistryStateError {
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::SchemaMismatch { .. } => ErrorCode::SchemaMismatch,
        }
    }

    /// Classify a registry state load/save failure.
    ///
    /// Anything that is not a recognised [`RegistryStateError`] is treated as
    /// corrupt state, matching the historical behaviour.
    pub fn classify(err: &anyhow::Error) -> ErrorCode {
        err.chain()
            .find_map(|cause| cause.downcast_ref::<Self>())
            .map_or(ErrorCode::StateCorrupt, Self::error_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_reports_schema_mismatch() {
        let err = anyhow::Error::from(RegistryStateError::SchemaMismatch {
            expected: 1,
            actual: 2,
        });
        assert_eq!(
            err.to_string(),
            "registry schema version mismatch: expected 1, got 2"
        );
        assert_eq!(
            RegistryStateError::classify(&err),
            ErrorCode::SchemaMismatch
        );
    }

    #[test]
    fn classify_sees_through_context() {
        let err = anyhow::Error::from(RegistryStateError::SchemaMismatch {
            expected: 1,
            actual: 7,
        })
        .context("failed to load registry snapshot");
        assert_eq!(
            RegistryStateError::classify(&err),
            ErrorCode::SchemaMismatch
        );
    }

    #[test]
    fn classify_defaults_to_state_corrupt() {
        let err = anyhow::anyhow!("failed to parse targets.json");
        assert_eq!(RegistryStateError::classify(&err), ErrorCode::StateCorrupt);
    }
}
