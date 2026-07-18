use serde_json::Value;

use crate::state_model::RegistryProjectionInstance;

use super::convergence::PreparedProjection;
use super::{ConvergenceMode, ExecutionMode, StandaloneMode};

impl ExecutionMode for StandaloneMode {
    const CONVERGENCE: bool = false;
    type Prepared = ();
    type Output = super::StandaloneProjectionExecutionOutput;

    fn none() {}
    fn prepared(_: PreparedProjection) {}
    fn output(
        projection: Option<RegistryProjectionInstance>,
        (): (),
        backup: Option<Value>,
        commit: Option<String>,
        meta: super::Meta,
        noop: bool,
    ) -> Self::Output {
        super::StandaloneProjectionExecutionOutput {
            projection,
            backup,
            commit,
            meta,
            noop,
        }
    }
}

impl ExecutionMode for ConvergenceMode {
    const CONVERGENCE: bool = true;
    type Prepared = Option<PreparedProjection>;
    type Output = super::ProjectionExecutionOutput;

    fn none() -> Self::Prepared {
        None
    }
    fn prepared(prepared: PreparedProjection) -> Self::Prepared {
        Some(prepared)
    }
    fn output(
        projection: Option<RegistryProjectionInstance>,
        prepared: Self::Prepared,
        backup: Option<Value>,
        commit: Option<String>,
        meta: super::Meta,
        noop: bool,
    ) -> Self::Output {
        super::ProjectionExecutionOutput {
            projection,
            prepared,
            backup,
            commit,
            meta,
            noop,
        }
    }
}
