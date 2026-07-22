use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::core::vocab::ProjectionMethod;
use crate::types::SyncState;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RegistryTransportState {
    #[serde(rename = "not_requested")]
    NotRequested,
    Synced,
    PendingPush,
    Diverged,
    Conflicted,
    LocalOnly,
    Error,
}

impl From<&SyncState> for RegistryTransportState {
    fn from(value: &SyncState) -> Self {
        match value {
            SyncState::Synced => Self::Synced,
            SyncState::PendingPush => Self::PendingPush,
            SyncState::Diverged => Self::Diverged,
            SyncState::Conflicted => Self::Conflicted,
            SyncState::LocalOnly => Self::LocalOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProjectionConvergenceState {
    Converged,
    Drifted,
    Missing,
    Conflict,
    NotApplicable,
    Unknown,
    Error,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VisibilityState {
    Visible,
    NotVisible,
    RestartRequired,
    Unsupported,
    Unknown,
    Error,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub(crate) struct AxisError {
    pub code: String,
    pub message: String,
}

impl AxisError {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RegistryTransportStatus {
    pub state: RegistryTransportState,
    pub evidence: Value,
    pub observed_at: DateTime<Utc>,
    pub stale: bool,
    pub errors: Vec<AxisError>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectionConvergenceItem {
    pub instance_id: String,
    pub skill_id: String,
    pub target_id: String,
    pub method: ProjectionMethod,
    pub state: ProjectionConvergenceState,
    pub source_digest: Option<String>,
    pub materialized_digest: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub errors: Vec<AxisError>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectionConvergenceStatus {
    pub state: ProjectionConvergenceState,
    pub items: Vec<ProjectionConvergenceItem>,
    pub evidence: Value,
    pub observed_at: DateTime<Utc>,
    pub stale: bool,
    pub errors: Vec<AxisError>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct VisibilityStatus {
    pub state: VisibilityState,
    pub agent: Option<String>,
    pub evidence: Value,
    pub observed_at: DateTime<Utc>,
    pub stale: bool,
    pub errors: Vec<AxisError>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConvergenceStatus {
    pub registry_transport: RegistryTransportStatus,
    pub projections: ProjectionConvergenceStatus,
    pub visibility: VisibilityStatus,
    pub observed_at: DateTime<Utc>,
    pub complete: bool,
    pub incomplete_axes: Vec<&'static str>,
}

impl ConvergenceStatus {
    pub(crate) fn refresh_completeness(&mut self) {
        let mut incomplete = Vec::new();
        if self.registry_transport.stale
            || self.registry_transport.state == RegistryTransportState::Error
        {
            incomplete.push("registry_transport");
        }
        if self.projections.stale
            || matches!(
                self.projections.state,
                ProjectionConvergenceState::Unknown | ProjectionConvergenceState::Error
            )
        {
            incomplete.push("projections");
        }
        if self.visibility.stale
            || matches!(
                self.visibility.state,
                VisibilityState::Unknown | VisibilityState::Error
            )
        {
            incomplete.push("visibility");
        }
        self.complete = incomplete.is_empty();
        self.incomplete_axes = incomplete;
    }

    pub(crate) fn mark_stale(&mut self, reason: &str) {
        self.mark_registry_transport_stale(reason);
        self.mark_projections_stale(reason);
        self.mark_visibility_stale(reason);
        self.refresh_completeness();
    }

    pub(crate) fn mark_registry_transport_stale(&mut self, reason: &str) {
        self.registry_transport.stale = true;
        self.registry_transport
            .errors
            .push(AxisError::new("evidence_changed_during_read", reason));
        self.refresh_completeness();
    }

    pub(crate) fn mark_projections_stale(&mut self, reason: &str) {
        self.projections.stale = true;
        self.projections
            .errors
            .push(AxisError::new("evidence_changed_during_read", reason));
        self.refresh_completeness();
    }

    pub(crate) fn mark_visibility_stale(&mut self, reason: &str) {
        self.visibility.stale = true;
        self.visibility
            .errors
            .push(AxisError::new("evidence_changed_during_read", reason));
        self.refresh_completeness();
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::{json, to_value};

    use super::*;

    fn fixture() -> ConvergenceStatus {
        let now = Utc::now();
        ConvergenceStatus {
            registry_transport: RegistryTransportStatus {
                state: RegistryTransportState::Synced,
                evidence: json!({"observed_revision": "abc"}),
                observed_at: now,
                stale: false,
                errors: Vec::new(),
            },
            projections: ProjectionConvergenceStatus {
                state: ProjectionConvergenceState::NotApplicable,
                items: Vec::new(),
                evidence: json!({"observed_revision": "abc"}),
                observed_at: now,
                stale: false,
                errors: Vec::new(),
            },
            visibility: VisibilityStatus {
                state: VisibilityState::Unsupported,
                agent: None,
                evidence: json!({"reason": "agent_not_selected"}),
                observed_at: now,
                stale: false,
                errors: Vec::new(),
            },
            observed_at: now,
            complete: false,
            incomplete_axes: Vec::new(),
        }
    }

    #[test]
    fn convergence_status_shape_has_three_named_axes() {
        let mut status = fixture();
        status.refresh_completeness();
        let value = to_value(status).expect("serialize convergence status");
        assert_eq!(value["registry_transport"]["state"], json!("SYNCED"));
        assert_eq!(value["projections"]["state"], json!("not_applicable"));
        assert_eq!(value["projections"]["items"], json!([]));
        assert_eq!(value["visibility"]["state"], json!("unsupported"));
        assert_eq!(value["complete"], json!(true));
    }

    #[test]
    fn convergence_status_partial_collection_names_incomplete_axes() {
        let mut status = fixture();
        status.visibility.state = VisibilityState::Unknown;
        status.visibility.errors.push(AxisError::new(
            "collection_interrupted",
            "visibility collection did not complete",
        ));
        status.refresh_completeness();
        assert!(!status.complete);
        assert_eq!(status.incomplete_axes, vec!["visibility"]);
    }

    #[test]
    fn convergence_status_marks_stale_on_race() {
        let mut status = fixture();
        status.mark_stale("revision changed");
        assert!(!status.complete);
        assert_eq!(
            status.incomplete_axes,
            vec!["registry_transport", "projections", "visibility"]
        );
        assert!(status.registry_transport.stale);
        assert_eq!(
            status.registry_transport.errors[0].code,
            "evidence_changed_during_read"
        );
    }
}
