use super::*;

#[must_use = "an activated projection must be finalized or rolled back"]
pub(crate) struct ProjectionActivationOutput {
    pub(super) projection: Option<RegistryProjectionInstance>,
    pub(super) rollback_artifact: Option<ProjectionRollbackArtifact>,
    pub(super) scope: Option<PreparedProjectionScope>,
    #[cfg(test)]
    pub(super) fail_cleanup_once: bool,
}

#[allow(
    dead_code,
    reason = "durable rollback evidence is consumed by the convergence transaction"
)]
impl ProjectionActivationOutput {
    pub(crate) fn projection(&self) -> &RegistryProjectionInstance {
        self.projection
            .as_ref()
            .expect("activated projection must own its projection identity")
    }

    pub(crate) fn rollback_evidence(&self) -> Value {
        self.rollback_artifact
            .as_ref()
            .expect("activated projection must own a rollback artifact")
            .evidence()
    }

    pub(crate) fn durable_rollback_artifact(&self) -> &ProjectionRollbackArtifact {
        self.rollback_artifact
            .as_ref()
            .expect("activated projection must own a rollback artifact")
    }

    pub(crate) fn into_durable_parts(
        mut self,
    ) -> (RegistryProjectionInstance, ProjectionRollbackArtifact) {
        let projection = self
            .projection
            .take()
            .expect("activated projection must own its projection identity");
        let artifact = self
            .rollback_artifact
            .take()
            .expect("activated projection must own a rollback artifact");
        (projection, artifact)
    }

    pub(crate) fn from_durable_parts(
        projection: RegistryProjectionInstance,
        artifact: ProjectionRollbackArtifact,
    ) -> Self {
        Self {
            projection: Some(projection),
            rollback_artifact: Some(artifact),
            scope: None,
            #[cfg(test)]
            fail_cleanup_once: false,
        }
    }

    pub(crate) fn rollback(&mut self) -> Result<(), CommandFailure> {
        let artifact = self.rollback_artifact.as_mut().ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                "activated projection has no rollback artifact",
            )
        })?;
        if let Some(scope) = self.scope.as_ref() {
            scope.prepare_rollback(artifact)?;
        } else {
            artifact.prepare_rollback()?;
        }
        self.projection = None;
        #[cfg(test)]
        if self.fail_cleanup_once {
            self.fail_cleanup_once = false;
            return Err(with_recovery_details(
                CommandFailure::new(
                    ErrorCode::InternalError,
                    "fault injected before rollback artifact cleanup",
                ),
                artifact,
            ));
        }
        if let Some(scope) = self.scope.as_ref() {
            scope.cleanup_pending(artifact)?;
        } else {
            artifact.cleanup_pending()?;
        }
        self.rollback_artifact = None;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fail_cleanup_once_for_test(&mut self) {
        self.fail_cleanup_once = true;
    }

    pub(crate) fn finalize(&mut self) -> Result<RegistryProjectionInstance, CommandFailure> {
        let artifact = self
            .rollback_artifact
            .as_mut()
            .expect("activated projection must own a rollback artifact");
        if matches!(
            artifact,
            ProjectionRollbackArtifact::PendingCleanup {
                reason: PendingCleanupReason::RollbackExchanged
                    | PendingCleanupReason::RollbackCreated,
                ..
            }
        ) {
            return Err(with_recovery_details(
                CommandFailure::new(
                    ErrorCode::ProjectionConflict,
                    "cannot finalize after rollback took effect; retry rollback cleanup",
                ),
                artifact,
            ));
        }
        if let Some(scope) = self.scope.as_ref() {
            scope.finalize(artifact)?;
        } else {
            artifact.finalize()?;
        }
        self.rollback_artifact = None;
        Ok(self
            .projection
            .take()
            .expect("activated projection must own its projection identity"))
    }
}

impl Drop for ProjectionActivationOutput {
    fn drop(&mut self) {
        if self.rollback_artifact.is_some()
            && let Err(err) = self.rollback()
        {
            eprintln!(
                "loom: abandoned projection activation requires recovery: {}; details={}",
                err.message, err.details
            );
        }
    }
}
