use std::collections::BTreeSet;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use crate::state_model::{RegistryOperationError, RegistryOperationRecord, RegistryStatePaths};

use super::AppContext;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct OperationCounts {
    pub actionable_operations: usize,
    pub local_journal_events: usize,
    pub unpushed_history_events: usize,
    pub local_only_history_events: usize,
}

impl OperationCounts {
    pub fn journal_events(&self) -> usize {
        self.actionable_operations + self.local_journal_events
    }

    pub fn history_events(&self) -> usize {
        self.unpushed_history_events + self.local_only_history_events
    }
}

#[derive(Debug, Clone, Default)]
pub struct RegistryOpsReport {
    pub ops: Vec<RegistryOperationRecord>,
    pub operation_counts: OperationCounts,
}

impl AppContext {
    pub fn read_registry_ops_report(&self) -> Result<RegistryOpsReport> {
        let paths = RegistryStatePaths::from_app_context(self);
        paths.ensure_layout()?;
        self.classify_registry_ops(paths.load_operations()?)
    }

    pub fn read_existing_registry_ops_report(&self) -> Result<RegistryOpsReport> {
        let paths = RegistryStatePaths::from_app_context(self);
        if !paths.exists() {
            return self.classify_registry_ops(Vec::new());
        }
        self.classify_registry_ops(paths.load_operations()?)
    }

    pub fn registry_operation_backlog_count(&self) -> Result<usize> {
        Ok(self
            .read_registry_ops_report()?
            .operation_counts
            .actionable_operations)
    }

    fn classify_registry_ops(
        &self,
        operations: Vec<RegistryOperationRecord>,
    ) -> Result<RegistryOpsReport> {
        let remote_configured = crate::gitops::remote_is_configured(self)?;
        let (ops, local_journal_events) = actionable_registry_ops(operations, remote_configured);
        let (unpushed_history_events, local_only_history_events) =
            crate::gitops::history_operation_counts(self, remote_configured)?;
        let operation_counts = OperationCounts {
            actionable_operations: ops.len(),
            local_journal_events,
            unpushed_history_events,
            local_only_history_events,
        };
        Ok(RegistryOpsReport {
            ops,
            operation_counts,
        })
    }

    pub fn ack_registry_ops(&self, op_ids: &BTreeSet<String>) -> Result<usize> {
        if op_ids.is_empty() {
            return Ok(0);
        }
        self.update_registry_ops(op_ids, RegistryOpsUpdate::Acked)
    }

    pub fn purge_registry_ops(&self) -> Result<usize> {
        let paths = RegistryStatePaths::from_app_context(self);
        paths.ensure_layout()?;
        let op_ids = unacknowledged_registry_ops(paths.load_operations()?)
            .into_iter()
            .map(|op| op.op_id)
            .collect::<BTreeSet<_>>();
        if op_ids.is_empty() {
            return Ok(0);
        }
        self.update_registry_ops(&op_ids, RegistryOpsUpdate::Purged)
    }

    pub fn fail_registry_ops(
        &self,
        op_ids: &BTreeSet<String>,
        code: &str,
        message: &str,
    ) -> Result<usize> {
        if op_ids.is_empty() {
            return Ok(0);
        }
        self.update_registry_ops(
            op_ids,
            RegistryOpsUpdate::Failed {
                code: code.to_string(),
                message: message.to_string(),
            },
        )
    }

    fn update_registry_ops(
        &self,
        op_ids: &BTreeSet<String>,
        update: RegistryOpsUpdate,
    ) -> Result<usize> {
        let paths = RegistryStatePaths::from_app_context(self);
        paths.ensure_layout()?;
        let mut operations = paths.load_operations()?;
        let now = Utc::now();
        let mut updated = 0usize;
        let mut last_updated = None;

        for op in &mut operations {
            if !op_ids.contains(&op.op_id) {
                continue;
            }
            match &update {
                RegistryOpsUpdate::Acked => {
                    op.ack = true;
                    op.status = "succeeded".to_string();
                    op.last_error = None;
                }
                RegistryOpsUpdate::Purged => {
                    op.ack = true;
                    op.status = "purged".to_string();
                    op.last_error = None;
                }
                RegistryOpsUpdate::Failed { code, message } => {
                    op.ack = false;
                    op.status = "failed".to_string();
                    op.last_error = Some(RegistryOperationError {
                        code: code.clone(),
                        message: message.clone(),
                    });
                }
            }
            op.updated_at = now;
            updated += 1;
            last_updated = Some(op.op_id.clone());
        }

        if updated == 0 {
            return Ok(0);
        }

        paths.save_operations(&operations)?;
        let mut checkpoint = paths.load_checkpoint()?;
        if matches!(update, RegistryOpsUpdate::Acked | RegistryOpsUpdate::Purged) {
            checkpoint.last_acked_op_id = last_updated;
        }
        checkpoint.updated_at = now;
        paths.save_checkpoint(&checkpoint)?;
        Ok(updated)
    }
}

fn actionable_registry_ops(
    operations: Vec<RegistryOperationRecord>,
    remote_configured: bool,
) -> (Vec<RegistryOperationRecord>, usize) {
    let mut local_journal_events = 0usize;
    let mut ops = unacknowledged_registry_ops(operations)
        .into_iter()
        .filter(|op| {
            if op.status == "succeeded" && !remote_configured {
                local_journal_events += 1;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();
    ops.sort_by_key(|op| op.created_at);
    (ops, local_journal_events)
}

fn unacknowledged_registry_ops(
    operations: Vec<RegistryOperationRecord>,
) -> Vec<RegistryOperationRecord> {
    operations
        .into_iter()
        .filter(|op| !op.ack && op.status != "purged")
        .collect()
}

enum RegistryOpsUpdate {
    Acked,
    Purged,
    Failed { code: String, message: String },
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    fn operation(status: &str, ack: bool, ordinal: i64) -> RegistryOperationRecord {
        let at = Utc::now() + chrono::Duration::seconds(ordinal);
        RegistryOperationRecord {
            op_id: format!("op-{ordinal}"),
            intent: "skill.project".to_string(),
            status: status.to_string(),
            ack,
            payload: json!({}),
            effects: json!({}),
            last_error: None,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn local_only_classifier_keeps_failures_and_unknown_states_actionable() {
        let (ops, local_journal_events) = actionable_registry_ops(
            vec![
                operation("succeeded", false, 1),
                operation("failed", false, 2),
                operation("pending", false, 3),
                operation("future_state", false, 4),
                operation("purged", false, 5),
                operation("failed", true, 6),
            ],
            false,
        );

        assert_eq!(local_journal_events, 1);
        assert_eq!(
            ops.iter().map(|op| op.status.as_str()).collect::<Vec<_>>(),
            vec!["failed", "pending", "future_state"]
        );
    }

    #[test]
    fn configured_remote_makes_succeeded_unacked_rows_actionable() {
        let (ops, local_journal_events) =
            actionable_registry_ops(vec![operation("succeeded", false, 1)], true);

        assert_eq!(local_journal_events, 0);
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].status, "succeeded");
    }
}
