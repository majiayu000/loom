mod errors;
mod json_io;
mod persistence;

pub use errors::RegistryStateError;

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::core::vocab::{AgentId, Health, MatcherKind, Ownership, ProjectionMethod};

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct RegistryStatePaths {
    pub root: PathBuf,
    pub state_dir: PathBuf,
    pub registry_dir: PathBuf,
    pub schema_file: PathBuf,
    pub targets_file: PathBuf,
    pub bindings_file: PathBuf,
    pub rules_file: PathBuf,
    pub projections_file: PathBuf,
    pub trust_file: PathBuf,
    pub ops_dir: PathBuf,
    pub operations_file: PathBuf,
    pub checkpoint_file: PathBuf,
    pub observations_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    pub schema: RegistrySchemaFile,
    pub targets: RegistryTargetsFile,
    pub bindings: RegistryBindingsFile,
    pub rules: RegistryRulesFile,
    pub projections: RegistryProjectionsFile,
    pub operations: Vec<RegistryOperationRecord>,
    pub checkpoint: RegistryOpsCheckpoint,
}

#[derive(Debug, Clone)]
pub struct RegistryTargetRelations<'a> {
    pub bindings: Vec<&'a RegistryWorkspaceBinding>,
    pub rules: Vec<&'a RegistryBindingRule>,
    pub projections: Vec<&'a RegistryProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySchemaFile {
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub writer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTargetsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub targets: Vec<RegistryProjectionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryBindingsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub bindings: Vec<RegistryWorkspaceBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryRulesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<RegistryBindingRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryProjectionsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub projections: Vec<RegistryProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTrustFile {
    pub schema_version: u32,
    #[serde(default)]
    pub skills: Vec<RegistryTrustRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTrustRecord {
    pub skill_id: String,
    pub trust: String,
    pub quarantined: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryTarget"
)]
pub struct RegistryProjectionTarget {
    pub target_id: String,
    #[ts(type = "string")]
    pub agent: AgentId,
    pub path: String,
    pub ownership: Ownership,
    pub capabilities: RegistryTargetCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/")]
pub struct RegistryTargetCapabilities {
    pub symlink: bool,
    pub copy: bool,
    pub watch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryBinding"
)]
pub struct RegistryWorkspaceBinding {
    pub binding_id: String,
    #[ts(type = "string")]
    pub agent: AgentId,
    pub profile_id: String,
    pub workspace_matcher: RegistryWorkspaceMatcher,
    pub default_target_id: String,
    pub policy_profile: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/")]
pub struct RegistryWorkspaceMatcher {
    pub kind: MatcherKind,
    pub value: String,
}

impl RegistryWorkspaceMatcher {
    /// Authoritative workspace-matching semantics, shared by every command so
    /// that projection, convergence, recommendation, and inspection all agree
    /// on which binding owns a given workspace.
    ///
    /// For the two path-based matcher kinds, both the workspace and the matcher
    /// value are canonicalized (resolving symlinks and relative components)
    /// before comparison. Only a missing suffix may fall back to the deepest
    /// existing ancestor; every other I/O error is returned to the caller.
    pub fn matches_workspace(&self, workspace: &std::path::Path) -> std::io::Result<bool> {
        use std::path::Path;

        match self.kind {
            MatcherKind::PathPrefix => Ok(canonicalize_workspace_path(workspace)?
                .starts_with(canonicalize_workspace_path(Path::new(&self.value))?)),
            MatcherKind::ExactPath => Ok(canonicalize_workspace_path(workspace)?
                == canonicalize_workspace_path(Path::new(&self.value))?),
            MatcherKind::Name => {
                Ok(workspace.file_name().and_then(|name| name.to_str())
                    == Some(self.value.as_str()))
            }
        }
    }
}

/// Normalize a workspace or matcher path so both sides of a comparison resolve
/// symlinks and relative components consistently.
///
/// Plain `fs::canonicalize` only works on paths that exist. A not-yet-created
/// workspace therefore canonicalizes its deepest existing ancestor and then
/// re-appends the missing suffix. This fallback is deliberately restricted to
/// `NotFound`; permission errors, symlink loops, and every other I/O failure
/// remain errors.
fn canonicalize_workspace_path(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let mut probe = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut suffix = Vec::new();
    loop {
        match std::fs::canonicalize(&probe) {
            Ok(mut canonical) => {
                for component in suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let Some(name) = probe.file_name() else {
                    return Err(workspace_path_error(path, err));
                };
                suffix.push(name.to_os_string());
                if !probe.pop() {
                    return Err(workspace_path_error(path, err));
                }
            }
            Err(err) => return Err(workspace_path_error(path, err)),
        }
    }
}

fn workspace_path_error(path: &std::path::Path, err: std::io::Error) -> std::io::Error {
    std::io::Error::new(
        err.kind(),
        format!(
            "failed to canonicalize workspace matcher path '{}': {err}",
            path.display()
        ),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../panel/src/generated/", rename = "RegistryRule")]
pub struct RegistryBindingRule {
    pub binding_id: String,
    pub skill_id: String,
    pub target_id: String,
    pub method: ProjectionMethod,
    pub watch_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryProjection"
)]
pub struct RegistryProjectionInstance {
    pub instance_id: String,
    pub skill_id: String,
    // `Some(id)` means the projection is owned by that binding; `None` means
    // the projection is orphaned after its binding was removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub binding_id: Option<String>,
    pub target_id: String,
    pub materialized_path: String,
    pub method: ProjectionMethod,
    pub last_applied_rev: String,
    pub health: Health,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub observed_drift: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub source_tree_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub materialized_tree_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub last_observed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub last_observed_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryOperationRecord {
    pub op_id: String,
    pub intent: String,
    pub status: String,
    pub ack: bool,
    pub payload: serde_json::Value,
    pub effects: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<RegistryOperationError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryOperationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(
    export,
    export_to = "../panel/src/generated/",
    rename = "RegistryCheckpoint"
)]
pub struct RegistryOpsCheckpoint {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_scanned_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_acked_op_id: Option<String>,
    #[ts(type = "string")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryObservationEvent {
    pub event_id: String,
    pub instance_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub observed_at: DateTime<Utc>,
}

pub(crate) fn empty_targets_file() -> RegistryTargetsFile {
    RegistryTargetsFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        targets: Vec::new(),
    }
}

pub(crate) fn empty_bindings_file() -> RegistryBindingsFile {
    RegistryBindingsFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        bindings: Vec::new(),
    }
}

pub(crate) fn empty_rules_file() -> RegistryRulesFile {
    RegistryRulesFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        rules: Vec::new(),
    }
}

pub(crate) fn empty_projections_file() -> RegistryProjectionsFile {
    RegistryProjectionsFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        projections: Vec::new(),
    }
}

pub(crate) fn empty_trust_file() -> RegistryTrustFile {
    RegistryTrustFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        skills: Vec::new(),
    }
}

use std::path::PathBuf;

impl RegistrySnapshot {
    pub fn status_view(&self) -> serde_json::Value {
        let mut unique_skills =
            HashSet::with_capacity(self.rules.rules.len() + self.projections.projections.len());
        for rule in &self.rules.rules {
            unique_skills.insert(rule.skill_id.as_str());
        }

        let mut drifted = 0;
        for projection in &self.projections.projections {
            unique_skills.insert(projection.skill_id.as_str());
            if projection_has_health_issue(projection) {
                drifted += 1;
            }
        }

        let active_bindings = self
            .bindings
            .bindings
            .iter()
            .filter(|binding| binding.active)
            .count();

        json!({
            "schema_version": self.schema.schema_version,
            "counts": {
                "skills": unique_skills.len(),
                "targets": self.targets.targets.len(),
                "bindings": self.bindings.bindings.len(),
                "active_bindings": active_bindings,
                "rules": self.rules.rules.len(),
                "projections": self.projections.projections.len(),
                "drifted_projections": drifted,
                "operations": self.operations.len()
            },
            "targets": self.targets.targets,
            "bindings": self.bindings.bindings,
            "rules": self.rules.rules,
            "projections": self
                .projections
                .projections
                .iter()
                .map(projection_status_view)
                .collect::<Vec<_>>(),
            "checkpoint": self.checkpoint
        })
    }

    pub fn binding(&self, binding_id: &str) -> Option<&RegistryWorkspaceBinding> {
        self.bindings
            .bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
    }

    pub fn target(&self, target_id: &str) -> Option<&RegistryProjectionTarget> {
        self.targets
            .targets
            .iter()
            .find(|target| target.target_id == target_id)
    }

    pub fn binding_default_target(
        &self,
        binding: &RegistryWorkspaceBinding,
    ) -> Option<RegistryProjectionTarget> {
        self.target(&binding.default_target_id).cloned()
    }

    pub fn binding_rules(&self, binding_id: &str) -> Vec<RegistryBindingRule> {
        self.rules
            .rules
            .iter()
            .filter(|rule| rule.binding_id == binding_id)
            .cloned()
            .collect()
    }

    pub fn binding_projections(&self, binding_id: &str) -> Vec<RegistryProjectionInstance> {
        self.projections
            .projections
            .iter()
            .filter(|projection| projection.binding_id.as_deref() == Some(binding_id))
            .cloned()
            .collect()
    }

    pub fn target_relations(&self, target_id: &str) -> RegistryTargetRelations<'_> {
        let mut rules = Vec::with_capacity(self.rules.rules.len());
        for rule in &self.rules.rules {
            if rule.target_id == target_id {
                rules.push(rule);
            }
        }

        let mut projections = Vec::with_capacity(self.projections.projections.len());
        for projection in &self.projections.projections {
            if projection.target_id == target_id {
                projections.push(projection);
            }
        }

        let mut linked_binding_ids = HashSet::with_capacity(rules.len() + projections.len());
        linked_binding_ids.extend(rules.iter().map(|rule| rule.binding_id.as_str()));
        linked_binding_ids.extend(
            projections
                .iter()
                .filter_map(|projection| projection.binding_id.as_deref()),
        );

        let mut bindings = Vec::with_capacity(self.bindings.bindings.len());
        for binding in &self.bindings.bindings {
            if binding.default_target_id == target_id
                || linked_binding_ids.contains(binding.binding_id.as_str())
            {
                bindings.push(binding);
            }
        }

        RegistryTargetRelations {
            bindings,
            rules,
            projections,
        }
    }
}

pub(crate) fn projection_observation_status(projection: &RegistryProjectionInstance) -> String {
    if let Some(error) = projection.last_observed_error.as_deref() {
        return match error {
            "digest_mismatch" | "symlink_target_mismatch" => "drifted",
            "materialized_missing" | "source_missing" => "missing",
            "materialized_unreadable" | "source_unreadable" | "not_symlink" => "unreadable",
            other => other,
        }
        .to_string();
    }
    if projection.observed_drift.unwrap_or(false) {
        return "drifted".to_string();
    }
    if projection.health != crate::core::vocab::Health::Healthy {
        return projection.health.as_str().to_string();
    }
    if matches!(
        projection.method,
        crate::core::vocab::ProjectionMethod::Copy
            | crate::core::vocab::ProjectionMethod::Materialize
    ) && (projection.source_tree_digest.is_none()
        || projection.materialized_tree_digest.is_none()
        || projection.last_observed_at.is_none())
    {
        return "not_observed".to_string();
    }
    "healthy".to_string()
}

pub(crate) fn projection_has_health_issue(projection: &RegistryProjectionInstance) -> bool {
    !matches!(
        projection_observation_status(projection).as_str(),
        "healthy" | "not_observed"
    )
}

fn projection_status_view(projection: &RegistryProjectionInstance) -> serde_json::Value {
    let mut value = json!(projection);
    value["observation_status"] = json!(projection_observation_status(projection));
    value
}

#[cfg(test)]
mod vocab_tests {
    use super::{
        RegistryBindingRule, RegistryProjectionInstance, RegistryProjectionTarget,
        RegistryWorkspaceMatcher,
    };

    #[test]
    fn registry_vocab_unknown_values_fail_deserialization() {
        let target = r#"{
            "target_id":"target_bad",
            "agent":"future-agent",
            "path":"/tmp/skills",
            "ownership":"typo",
            "capabilities":{"symlink":true,"copy":true,"watch":true}
        }"#;
        assert!(serde_json::from_str::<RegistryProjectionTarget>(target).is_err());

        let matcher = r#"{"kind":"typo","value":"/tmp/work"}"#;
        assert!(serde_json::from_str::<RegistryWorkspaceMatcher>(matcher).is_err());

        let rule = r#"{
            "binding_id":"bind",
            "skill_id":"demo",
            "target_id":"target",
            "method":"typo",
            "watch_policy":"observe_only"
        }"#;
        assert!(serde_json::from_str::<RegistryBindingRule>(rule).is_err());

        let projection = r#"{
            "instance_id":"inst",
            "skill_id":"demo",
            "target_id":"target",
            "materialized_path":"/tmp/skills/demo",
            "method":"copy",
            "last_applied_rev":"abc123",
            "health":"typo"
        }"#;
        assert!(serde_json::from_str::<RegistryProjectionInstance>(projection).is_err());
    }

    #[test]
    fn registry_agent_field_preserves_unknown_reader_values() {
        let target = r#"{
            "target_id":"target_future",
            "agent":"future-agent",
            "path":"/tmp/skills",
            "ownership":"external",
            "capabilities":{"symlink":false,"copy":false,"watch":false}
        }"#;
        let parsed = serde_json::from_str::<RegistryProjectionTarget>(target)
            .expect("unknown agent remains reader-compatible");
        assert_eq!(parsed.agent, "future-agent");
    }
}

#[cfg(all(test, unix))]
mod matches_workspace_tests {
    use super::{MatcherKind, RegistryWorkspaceMatcher};
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    fn matcher(kind: MatcherKind, value: &str) -> RegistryWorkspaceMatcher {
        RegistryWorkspaceMatcher {
            kind,
            value: value.to_string(),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "loom_match_{tag}_{}",
            uuid::Uuid::new_v4().simple()
        ))
    }

    // Regression: a workspace reached through a symlink must match a matcher that
    // points at the real path. Before unification, the recommend/inspect copies
    // used a raw `starts_with` and would NOT match here, while the projection
    // side (canonicalize) would — so the two disagreed on binding ownership.
    #[test]
    fn path_prefix_matches_through_symlinked_workspace() {
        let base = scratch("prefix");
        let real = base.join("real");
        std::fs::create_dir_all(real.join("proj")).expect("create real tree");
        let link = base.join("link");
        symlink(&real, &link).expect("create symlink");

        let workspace_via_link = link.join("proj");
        let m = matcher(MatcherKind::PathPrefix, real.to_str().unwrap());

        // Documents the drift this fix removes: the raw form does not match.
        assert!(
            !workspace_via_link.starts_with(&real),
            "raw path comparison would not match through the symlink"
        );
        // The unified, canonicalizing implementation resolves the symlink and matches.
        assert!(
            m.matches_workspace(&workspace_via_link)
                .expect("match workspace"),
            "canonicalized matcher must match the symlinked workspace"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn name_matches_final_component() {
        let m = matcher(MatcherKind::Name, "my-workspace");
        assert!(
            m.matches_workspace(std::path::Path::new("/home/x/my-workspace"))
                .expect("name match")
        );
        assert!(
            !m.matches_workspace(std::path::Path::new("/home/x/other"))
                .expect("name mismatch")
        );
    }

    #[test]
    fn non_matching_prefix_is_rejected() {
        let base = scratch("reject");
        std::fs::create_dir_all(base.join("a")).expect("create tree");
        std::fs::create_dir_all(base.join("b")).expect("create tree");
        let m = matcher(MatcherKind::PathPrefix, base.join("a").to_str().unwrap());
        assert!(
            !m.matches_workspace(&base.join("b"))
                .expect("non-matching prefix")
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn relative_trailing_slash_and_missing_suffix_normalize_consistently() {
        let base = scratch("relative");
        let existing = base.join("existing");
        std::fs::create_dir_all(&existing).expect("create existing ancestor");
        let current = std::env::current_dir().expect("current dir");
        let mut relative = PathBuf::new();
        for component in current.components() {
            if matches!(component, std::path::Component::Normal(_)) {
                relative.push("..");
            }
        }
        for component in existing.components() {
            if let std::path::Component::Normal(component) = component {
                relative.push(component);
            }
        }
        let matcher_value = existing.join("future").join("child");
        let with_trailing_slash = PathBuf::from(format!("{}/", matcher_value.display()));
        let m = matcher(
            MatcherKind::ExactPath,
            with_trailing_slash.to_str().expect("utf8 test path"),
        );

        let workspace = relative.join("future").join("child");
        assert!(
            m.matches_workspace(&workspace)
                .expect("missing suffix match")
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn permission_denied_is_not_silently_treated_as_a_non_match() {
        let base = scratch("permission");
        let denied = base.join("denied");
        std::fs::create_dir_all(denied.join("child")).expect("create denied tree");
        std::fs::set_permissions(&denied, Permissions::from_mode(0o000))
            .expect("remove permissions");
        let m = matcher(
            MatcherKind::ExactPath,
            denied.join("child").to_str().expect("utf8 test path"),
        );

        let result = m.matches_workspace(&denied.join("child"));

        std::fs::set_permissions(&denied, Permissions::from_mode(0o700))
            .expect("restore permissions");
        std::fs::remove_dir_all(&base).ok();
        let err = result.expect_err("permission failure must propagate");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn symlink_loop_is_not_silently_treated_as_a_non_match() {
        let base = scratch("loop");
        std::fs::create_dir_all(&base).expect("create loop root");
        let left = base.join("left");
        let right = base.join("right");
        symlink(&right, &left).expect("create first loop edge");
        symlink(&left, &right).expect("create second loop edge");
        let m = matcher(
            MatcherKind::ExactPath,
            left.to_str().expect("utf8 test path"),
        );

        let err = m
            .matches_workspace(&left)
            .expect_err("symlink loop must propagate");
        assert_ne!(err.kind(), std::io::ErrorKind::NotFound);

        std::fs::remove_file(&left).ok();
        std::fs::remove_file(&right).ok();
        std::fs::remove_dir_all(&base).ok();
    }
}
