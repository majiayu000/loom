use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub(super) const TELEMETRY_SCHEMA_VERSION: u32 = 1;
pub(super) const TELEMETRY_EVENT_SCHEMA_VERSION: u32 = 2;
pub(super) const DEFAULT_RETENTION_DAYS: u32 = 90;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TelemetryConfig {
    pub schema_version: u32,
    pub enabled: bool,
    pub mode: TelemetryMode,
    pub redaction: TelemetryRedaction,
    pub retention_days: u32,
}

impl TelemetryConfig {
    pub(super) fn enabled_local() -> Self {
        Self {
            schema_version: TELEMETRY_SCHEMA_VERSION,
            enabled: true,
            mode: TelemetryMode::LocalOnly,
            redaction: TelemetryRedaction::Default,
            retention_days: DEFAULT_RETENTION_DAYS,
        }
    }

    pub(super) fn disabled_local() -> Self {
        Self {
            enabled: false,
            ..Self::enabled_local()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum TelemetryMode {
    #[serde(rename = "local-only")]
    LocalOnly,
}

impl TelemetryMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TelemetryRedaction {
    Default,
}

impl TelemetryRedaction {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TelemetryEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub event_type: TelemetryEventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skillset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_hash: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub metrics: TelemetryMetrics,
    pub privacy: TelemetryPrivacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) enum TelemetryEventType {
    #[serde(rename = "skill.activation")]
    SkillActivation,
    #[serde(rename = "skill.deactivation")]
    SkillDeactivation,
    #[serde(rename = "skill.invocation")]
    SkillInvocation,
    #[serde(rename = "skill.eval")]
    SkillEval,
    #[serde(rename = "skill.safety")]
    SkillSafety,
    #[serde(rename = "skill.error")]
    SkillError,
    #[serde(rename = "recommendation.feedback")]
    RecommendationFeedback,
}

impl TelemetryEventType {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::SkillActivation => "skill.activation",
            Self::SkillDeactivation => "skill.deactivation",
            Self::SkillInvocation => "skill.invocation",
            Self::SkillEval => "skill.eval",
            Self::SkillSafety => "skill.safety",
            Self::SkillError => "skill.error",
            Self::RecommendationFeedback => "recommendation.feedback",
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TelemetryMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_out: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_delta: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<RecommendationFeedback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_findings: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_findings: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<String>,
}

impl TelemetryMetrics {
    pub(super) fn has_cost(&self) -> bool {
        self.tokens_in.is_some()
            || self.tokens_out.is_some()
            || self.commands.is_some()
            || self.duration_ms.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RecommendationFeedback {
    Accepted,
    Rejected,
    Ignored,
}

impl RecommendationFeedback {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Ignored => "ignored",
        }
    }
}

pub(crate) fn failure_category_allowed(value: &str) -> bool {
    matches!(
        value,
        "timeout"
            | "tool_error"
            | "model_error"
            | "dependency_error"
            | "permission_denied"
            | "rate_limited"
            | "invalid_input"
            | "policy_blocked"
            | "not_found"
            | "network_error"
            | "execution_error"
            | "unknown"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TelemetryPrivacy {
    pub raw_prompt_stored: bool,
    pub raw_code_stored: bool,
    pub redacted: bool,
}

impl Default for TelemetryPrivacy {
    fn default() -> Self {
        Self {
            raw_prompt_stored: false,
            raw_code_stored: false,
            redacted: true,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TelemetryEventDraft {
    pub(crate) event_type: TelemetryEventType,
    pub(crate) skill_id: Option<String>,
    pub(crate) skillset_id: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) workspace: Option<PathBuf>,
    pub(crate) session_id: Option<String>,
    pub(crate) task: Option<String>,
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) metrics: TelemetryMetrics,
}

impl TelemetryEventDraft {
    pub(crate) fn new(event_type: TelemetryEventType) -> Self {
        Self {
            event_type,
            skill_id: None,
            skillset_id: None,
            agent: None,
            workspace: None,
            session_id: None,
            task: None,
            timestamp: Utc::now(),
            metrics: TelemetryMetrics::default(),
        }
    }
}
