use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const COMPILE_SCHEMA_VERSION: u32 = 1;
pub(super) const COMPILER_VERSION: &str = "loom-compiled-v1";
pub(super) const MIN_SOURCE_TOKENS: usize = 1500;

pub(super) const REQUIRED_ARTIFACT_FILES: [(&str, &str); 5] = [
    ("activation.md", "activation_md"),
    ("catalog.json", "catalog_json"),
    ("boundaries.json", "boundaries_json"),
    ("tool-interface.json", "tool_interface_json"),
    ("references.index.json", "references_index_json"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ArtifactStatus {
    Planned,
    Experimental,
    Valid,
    Stale,
    Blocked,
    Invalid,
}

impl ArtifactStatus {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Experimental => "experimental",
            Self::Valid => "valid",
            Self::Stale => "stale",
            Self::Blocked => "blocked",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum GateValue {
    Pass,
    Warning,
    Missing,
    Blocked,
    Fail,
}

impl GateValue {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warning => "warning",
            Self::Missing => "missing",
            Self::Blocked => "blocked",
            Self::Fail => "fail",
        }
    }

    pub(super) fn allows_valid(self) -> bool {
        self == Self::Pass
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompileGateStatus {
    pub lint: GateValue,
    pub safety: GateValue,
    pub dependency: GateValue,
    pub eval: GateValue,
}

impl CompileGateStatus {
    pub(super) fn all_pass(&self) -> bool {
        self.lint.allows_valid()
            && self.safety.allows_valid()
            && self.dependency.allows_valid()
            && self.eval.allows_valid()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompileTokenEstimate {
    pub source_skill_md: usize,
    pub activation_md: usize,
    pub compiled_runtime_total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompiledArtifactManifest {
    pub schema_version: u32,
    pub artifact_id: String,
    pub skill: String,
    pub agent: String,
    pub profile: String,
    pub source_ref: String,
    pub source_tree_oid: Option<String>,
    pub source_digest: String,
    pub compiler_version: String,
    pub status: ArtifactStatus,
    pub gates: CompileGateStatus,
    pub content_hashes: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_evidence: Option<CompileEvalEvidence>,
    pub token_estimate: CompileTokenEstimate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompileEvalEvidence {
    pub mode: String,
    pub agent: String,
    pub eval_suite_digest: String,
    pub report_digest: String,
    pub generated_content_hashes: BTreeMap<String, String>,
    pub summary: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct SourceDigestInput {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
}

pub(super) struct SourceDigestInfo {
    pub digest: String,
    pub inputs: Vec<SourceDigestInput>,
}

pub(super) struct GatePlan {
    pub manifest: CompileGateStatus,
    pub details: Value,
    pub eval_evidence: Option<CompileEvalEvidence>,
}

impl GatePlan {
    pub(super) fn has_blocking_failure(&self) -> bool {
        matches!(self.manifest.lint, GateValue::Fail | GateValue::Blocked)
            || matches!(self.manifest.safety, GateValue::Fail | GateValue::Blocked)
            || matches!(
                self.manifest.dependency,
                GateValue::Fail | GateValue::Blocked
            )
    }
}

pub(super) struct PlannedArtifact {
    pub artifact_id: String,
    pub artifact_root: PathBuf,
    pub paths: BTreeMap<String, String>,
    pub content_hashes: BTreeMap<String, String>,
    pub token_estimate: CompileTokenEstimate,
    pub no_op: bool,
    pub no_op_reason: Option<String>,
    pub content: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogDoc {
    pub schema_version: u32,
    pub sections: Vec<CatalogSection>,
    pub sidecars: Vec<CatalogSidecar>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogSection {
    pub id: String,
    pub title: String,
    pub content_hash: String,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogSidecar {
    pub name: String,
    pub schema: String,
    pub content_hash: String,
    pub required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundariesDoc {
    pub schema_version: u32,
    pub triggers: Vec<String>,
    pub non_triggers: Vec<String>,
    pub deferred_operations: Vec<String>,
    pub required_handoff_fields: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolInterfaceDoc {
    pub schema_version: u32,
    pub allowed_tools: Vec<ToolRecord>,
    pub script_entrypoints: Vec<ScriptRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolRecord {
    pub name: String,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScriptRecord {
    pub path: String,
    pub usage: String,
    pub risk: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferencesDoc {
    pub schema_version: u32,
    pub references: Vec<ReferenceRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferenceRecord {
    pub path: String,
    pub role: String,
    pub load_condition: String,
    pub content_hash: String,
}

#[derive(Debug, Serialize)]
pub(super) struct ArtifactVerifyReport {
    pub artifact_id: String,
    pub path: String,
    pub valid: bool,
    pub status: String,
    pub source_stale: bool,
    pub findings: Vec<VerifyFinding>,
    pub manifest: Option<CompiledArtifactManifest>,
}

#[derive(Debug, Serialize)]
pub(super) struct VerifyFinding {
    pub id: String,
    pub severity: String,
    pub message: String,
    pub details: Value,
}
