use serde_json::{Value, json};

use crate::envelope::Meta;
use crate::state_model::{
    RegistryBindingsFile, RegistryProjectionsFile, RegistryRulesFile, RegistryStatePaths,
    RegistryTargetsFile,
};

use super::helpers::{commit_registry_state, map_registry_state};
use super::projections::{maybe_autosync_or_queue, record_registry_operation};
use super::skill_cmds::shared::{maybe_push_rollback_fault, push_rollback_error};
use super::{App, CommandFailure};

/// Registry state written by [`App::registry_write_txn`], paired with the state
/// to restore when the operation record cannot be persisted. Pairing both sides
/// in one variant guarantees the rollback writes the same surface as the save.
pub(crate) enum RegistryTxnState {
    Targets {
        next: RegistryTargetsFile,
        original: RegistryTargetsFile,
    },
    Bindings {
        next: RegistryBindingsFile,
        original: RegistryBindingsFile,
    },
    Projections {
        next: RegistryProjectionsFile,
        original: RegistryProjectionsFile,
    },
    BindingsRulesProjections {
        next: (
            RegistryBindingsFile,
            RegistryRulesFile,
            RegistryProjectionsFile,
        ),
        original: (
            RegistryBindingsFile,
            RegistryRulesFile,
            RegistryProjectionsFile,
        ),
    },
}

impl RegistryTxnState {
    fn save_next(&self, paths: &RegistryStatePaths) -> anyhow::Result<()> {
        match self {
            Self::Targets { next, .. } => paths.save_targets(next),
            Self::Bindings { next, .. } => paths.save_bindings(next),
            Self::Projections { next, .. } => paths.save_projections(next),
            Self::BindingsRulesProjections {
                next: (bindings, rules, projections),
                ..
            } => paths.save_bindings_rules_projections(bindings, rules, projections),
        }
    }

    fn restore_original(&self, paths: &RegistryStatePaths) -> anyhow::Result<()> {
        match self {
            Self::Targets { original, .. } => paths.save_targets(original),
            Self::Bindings { original, .. } => paths.save_bindings(original),
            Self::Projections { original, .. } => paths.save_projections(original),
            Self::BindingsRulesProjections {
                original: (bindings, rules, projections),
                ..
            } => paths.save_bindings_rules_projections(bindings, rules, projections),
        }
    }
}

/// Registry commit + autosync step of a write transaction. Commands that never
/// commit registry state (for example `skill orphan clean`) omit it.
pub(crate) struct RegistryTxnCommit {
    pub message: String,
    /// Autosync payload; the commit id is inserted under `"commit"` by the txn.
    pub autosync_payload: Value,
}

pub(crate) struct RegistryWriteTxn<'a> {
    /// Operation intent, also used as the autosync command name.
    pub op_name: &'a str,
    pub request_id: &'a str,
    pub state: RegistryTxnState,
    pub op_payload: Value,
    pub op_effects: Value,
    /// Extra context prepended to the operation-log error when parts of the
    /// mutation are irrecoverable (for example already-deleted live paths).
    pub op_failure_note: Option<String>,
    pub commit: Option<RegistryTxnCommit>,
}

/// Outcome of a registry write transaction. The recorded `op_id` is carried in
/// `meta.op_id`.
pub(crate) struct RegistryTxnOutcome {
    pub commit: Option<String>,
    pub meta: Meta,
}

impl App {
    /// Shared tail of a registry write transaction: save the mutated state,
    /// record the registry operation (restoring the original state with
    /// visible rollback errors when the record cannot be persisted), commit
    /// the registry state, and autosync. Command-specific locking, validation,
    /// and mutation logic stay at the call site.
    pub(crate) fn registry_write_txn(
        &self,
        paths: &RegistryStatePaths,
        txn: RegistryWriteTxn<'_>,
    ) -> std::result::Result<RegistryTxnOutcome, CommandFailure> {
        txn.state.save_next(paths).map_err(map_registry_state)?;

        let op_id =
            match record_registry_operation(paths, txn.op_name, txn.op_payload, txn.op_effects) {
                Ok(op_id) => op_id,
                Err(mut err) => {
                    let mut rollback_errors = Vec::new();
                    if !maybe_push_rollback_fault(&mut rollback_errors, "restore_registry_state")
                        && let Err(restore_err) = txn.state.restore_original(paths)
                    {
                        push_rollback_error(
                            &mut rollback_errors,
                            "restore_registry_state",
                            restore_err,
                        );
                    }
                    if let Some(note) = txn.op_failure_note {
                        err = err.context(note);
                    }
                    return Err(map_registry_state(err).with_rollback_errors(rollback_errors));
                }
            };

        let mut meta = Meta {
            op_id: Some(op_id),
            ..Meta::default()
        };
        let Some(commit_step) = txn.commit else {
            return Ok(RegistryTxnOutcome { commit: None, meta });
        };
        let commit = commit_registry_state(&self.ctx, &commit_step.message)?;
        if let Some(commit_id) = &commit {
            let mut payload = commit_step.autosync_payload;
            payload["commit"] = json!(commit_id);
            maybe_autosync_or_queue(&self.ctx, txn.op_name, txn.request_id, payload, &mut meta)?;
        }
        Ok(RegistryTxnOutcome { commit, meta })
    }
}
