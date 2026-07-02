use std::collections::BTreeSet;

use anyhow::Result;
use chrono::Utc;

use crate::state_model::{RegistryOperationError, RegistryOperationRecord, RegistryStatePaths};

use super::AppContext;

#[derive(Debug, Clone, Default)]
pub struct RegistryOpsReport {
    pub ops: Vec<RegistryOperationRecord>,
}

impl AppContext {
    pub fn read_registry_ops_report(&self) -> Result<RegistryOpsReport> {
        let paths = RegistryStatePaths::from_app_context(self);
        paths.ensure_layout()?;
        let ops = active_registry_ops(paths.load_operations()?);
        Ok(RegistryOpsReport { ops })
    }

    pub fn read_existing_registry_ops_report(&self) -> Result<RegistryOpsReport> {
        let paths = RegistryStatePaths::from_app_context(self);
        if !paths.exists() {
            return Ok(RegistryOpsReport::default());
        }
        let ops = active_registry_ops(paths.load_operations()?);
        Ok(RegistryOpsReport { ops })
    }

    pub fn existing_registry_pending_count(&self) -> Result<usize> {
        Ok(self.read_existing_registry_ops_report()?.ops.len())
    }

    pub fn registry_or_pending_count(&self) -> Result<usize> {
        let registry = self.existing_registry_pending_count()?;
        if registry > 0 {
            return Ok(registry);
        }
        Ok(self.read_pending_report()?.ops.len())
    }

    pub fn read_registry_or_pending_ops_report(&self) -> Result<RegistryOrPendingOpsReport> {
        let pending = self.read_pending_report()?;
        if !pending.ops.is_empty() || !pending.warnings.is_empty() || pending.history_events > 0 {
            return Ok(RegistryOrPendingOpsReport::Pending(pending));
        }
        let registry = self.read_existing_registry_ops_report()?;
        if !registry.ops.is_empty() {
            return Ok(RegistryOrPendingOpsReport::Registry(registry));
        }
        Ok(RegistryOrPendingOpsReport::Pending(pending))
    }

    pub fn registry_pending_count(&self) -> Result<usize> {
        Ok(self.read_registry_ops_report()?.ops.len())
    }

    pub fn ack_registry_ops(&self, op_ids: &BTreeSet<String>) -> Result<usize> {
        if op_ids.is_empty() {
            return Ok(0);
        }
        self.update_registry_ops(op_ids, RegistryOpsUpdate::Acked)
    }

    pub fn purge_registry_ops(&self) -> Result<usize> {
        let report = self.read_registry_ops_report()?;
        let op_ids = report
            .ops
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

#[derive(Debug, Clone)]
pub enum RegistryOrPendingOpsReport {
    Registry(RegistryOpsReport),
    Pending(super::PendingOpsReport),
}

fn active_registry_ops(operations: Vec<RegistryOperationRecord>) -> Vec<RegistryOperationRecord> {
    let mut ops = operations
        .into_iter()
        .filter(|op| !op.ack && op.status != "purged")
        .collect::<Vec<_>>();
    ops.sort_by_key(|op| op.created_at);
    ops
}

enum RegistryOpsUpdate {
    Acked,
    Purged,
    Failed { code: String, message: String },
}
