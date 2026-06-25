use serde_json::{Value, json};

use super::{
    CheckResult, EvalRun, EvalStatus, EvalSummary, SkillEvalVersion, TaskMetrics, TaskResult,
    TriggerResult,
};

impl SkillEvalVersion {
    pub(super) fn to_value(&self) -> Value {
        json!({
            "head_tree_oid": self.head_tree_oid,
            "last_source_commit": self.last_source_commit,
        })
    }
}

impl TaskMetrics {
    fn to_value(&self) -> Value {
        json!({
            "tokens": self.tokens,
            "commands": self.commands,
            "duration_ms": self.duration_ms,
        })
    }
}

impl EvalStatus {
    fn json_label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl EvalSummary {
    pub(super) fn to_value(&self) -> Value {
        json!({
            "case_count": self.case_count,
            "passed": self.passed,
            "failed": self.failed,
            "skipped": self.skipped,
            "aggregate_score": self.aggregate_score,
            "trigger_precision": self.trigger_precision,
            "trigger_recall": self.trigger_recall,
            "task_success_rate": self.task_success_rate,
            "token_count": self.token_count,
            "command_count": self.command_count,
            "permissions_used": self.permissions_used,
        })
    }
}

impl EvalRun {
    pub(super) fn to_value(&self) -> Value {
        json!({
            "agent": self.agent,
            "model": self.model,
            "mode": self.mode,
            "summary": self.summary.to_value(),
            "triggers": self.triggers.iter().map(TriggerResult::to_value).collect::<Vec<_>>(),
            "tasks": self.tasks.iter().map(TaskResult::to_value).collect::<Vec<_>>(),
        })
    }
}

impl TriggerResult {
    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "line": self.line,
            "prompt": self.prompt,
            "expected_trigger": self.expected_trigger,
            "observed_trigger": self.observed_trigger,
            "status": self.status.json_label(),
            "score": self.score,
            "grader": self.grader,
        })
    }
}

impl TaskResult {
    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "line": self.line,
            "task": self.task,
            "status": self.status.json_label(),
            "score": self.score,
            "grader": self.grader,
            "metrics": self.metrics.to_value(),
            "permissions_used": self.permissions_used,
            "checks": self.checks.iter().map(CheckResult::to_value).collect::<Vec<_>>(),
        })
    }
}

impl CheckResult {
    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "status": self.status.json_label(),
            "message": self.message,
            "details": self.details,
        })
    }
}
