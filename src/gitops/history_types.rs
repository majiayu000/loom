use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRepairStrategy {
    Local,
    Remote,
}

impl HistoryRepairStrategy {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HistoryStatusReport {
    pub local_branch: bool,
    pub remote_tracking: bool,
    pub ahead: u32,
    pub behind: u32,
    pub local_segments: usize,
    pub local_archives: usize,
    pub remote_segments: usize,
    pub remote_archives: usize,
    pub local_snapshot: bool,
    pub remote_snapshot: bool,
    pub compact_after_segments: usize,
    pub retain_recent_segments: usize,
    pub retain_archives: usize,
    pub conflicts: Vec<HistoryConflictReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryConflictReport {
    pub scope: String,
    pub path: String,
    pub local_blob: String,
    pub remote_blob: String,
    pub local_rename_path: String,
    pub remote_rename_path: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HistoryRepairReport {
    pub result: String,
    pub strategy: String,
    pub commit: Option<String>,
    pub repaired_conflicts: usize,
    pub compacted_segments: usize,
    pub rolled_archives: usize,
    pub local_segments: usize,
    pub local_archives: usize,
    pub local_snapshot: bool,
    pub conflicts: Vec<HistoryConflictReport>,
}

impl HistoryRepairReport {
    pub(crate) fn noop(strategy: HistoryRepairStrategy) -> Self {
        Self {
            result: "noop".to_string(),
            strategy: strategy.as_str().to_string(),
            ..Self::default()
        }
    }

    pub(crate) fn from_status(
        result: impl Into<String>,
        strategy: HistoryRepairStrategy,
        commit: Option<String>,
        status: &HistoryStatusReport,
    ) -> Self {
        Self::from_counts(
            result,
            strategy,
            commit,
            status.local_segments,
            status.local_archives,
            status.local_snapshot,
        )
    }

    pub(crate) fn from_counts(
        result: impl Into<String>,
        strategy: HistoryRepairStrategy,
        commit: Option<String>,
        local_segments: usize,
        local_archives: usize,
        local_snapshot: bool,
    ) -> Self {
        Self {
            result: result.into(),
            strategy: strategy.as_str().to_string(),
            commit,
            local_segments,
            local_archives,
            local_snapshot,
            ..Self::default()
        }
    }
}
