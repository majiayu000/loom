#![allow(dead_code)]

use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const V3_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone)]
pub struct V3StatePaths {
    pub root: PathBuf,
    pub state_dir: PathBuf,
    pub v3_dir: PathBuf,
    pub schema_file: PathBuf,
    pub targets_file: PathBuf,
    pub bindings_file: PathBuf,
    pub rules_file: PathBuf,
    pub projections_file: PathBuf,
    pub ops_dir: PathBuf,
    pub operations_file: PathBuf,
    pub checkpoint_file: PathBuf,
    pub observations_dir: PathBuf,
}

impl V3StatePaths {
    pub fn from_root(root: &Path) -> Self {
        let state_dir = root.join("state");
        let v3_dir = state_dir.join("v3");
        let ops_dir = v3_dir.join("ops");
        let observations_dir = v3_dir.join("observations");
        Self {
            root: root.to_path_buf(),
            state_dir,
            v3_dir: v3_dir.clone(),
            schema_file: v3_dir.join("schema.json"),
            targets_file: v3_dir.join("targets.json"),
            bindings_file: v3_dir.join("bindings.json"),
            rules_file: v3_dir.join("rules.json"),
            projections_file: v3_dir.join("projections.json"),
            ops_dir: ops_dir.clone(),
            operations_file: ops_dir.join("operations.jsonl"),
            checkpoint_file: ops_dir.join("checkpoint.json"),
            observations_dir,
        }
    }

    pub fn exists(&self) -> bool {
        self.schema_file.exists()
    }

    pub fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(&self.v3_dir)
            .with_context(|| format!("failed to create {}", self.v3_dir.display()))?;
        fs::create_dir_all(&self.ops_dir)
            .with_context(|| format!("failed to create {}", self.ops_dir.display()))?;
        fs::create_dir_all(&self.observations_dir)
            .with_context(|| format!("failed to create {}", self.observations_dir.display()))?;

        ensure_json_file(
            &self.schema_file,
            &V3SchemaFile {
                schema_version: V3_SCHEMA_VERSION,
                created_at: Utc::now(),
                writer: format!("loom/{}", env!("CARGO_PKG_VERSION")),
            },
        )?;
        ensure_json_file(&self.targets_file, &empty_targets_file())?;
        ensure_json_file(&self.bindings_file, &empty_bindings_file())?;
        ensure_json_file(&self.rules_file, &empty_rules_file())?;
        ensure_json_file(&self.projections_file, &empty_projections_file())?;
        ensure_json_file(
            &self.checkpoint_file,
            &V3OpsCheckpoint {
                schema_version: V3_SCHEMA_VERSION,
                last_scanned_op_id: None,
                last_acked_op_id: None,
                updated_at: Utc::now(),
            },
        )?;
        ensure_text_file(&self.operations_file, "")?;
        Ok(())
    }

    pub fn load_or_init_snapshot(&self) -> Result<V3Snapshot> {
        self.ensure_layout()?;
        self.load_snapshot()
    }

    pub fn load_snapshot(&self) -> Result<V3Snapshot> {
        let schema = self.load_schema()?;
        validate_schema_version(schema.schema_version)?;
        Ok(V3Snapshot {
            schema,
            targets: self.load_targets()?,
            bindings: self.load_bindings()?,
            rules: self.load_rules()?,
            projections: self.load_projections()?,
            operations: self.load_operations()?,
            checkpoint: self.load_checkpoint()?,
        })
    }

    pub fn maybe_load_snapshot(&self) -> Result<Option<V3Snapshot>> {
        if !self.exists() {
            return Ok(None);
        }
        self.load_snapshot().map(Some)
    }

    pub fn load_schema(&self) -> Result<V3SchemaFile> {
        read_json_file(&self.schema_file)
    }

    pub fn load_targets(&self) -> Result<V3TargetsFile> {
        read_json_file(&self.targets_file)
    }

    pub fn load_bindings(&self) -> Result<V3BindingsFile> {
        read_json_file(&self.bindings_file)
    }

    pub fn load_rules(&self) -> Result<V3RulesFile> {
        read_json_file(&self.rules_file)
    }

    pub fn load_projections(&self) -> Result<V3ProjectionsFile> {
        read_json_file(&self.projections_file)
    }

    pub fn load_operations(&self) -> Result<Vec<V3OperationRecord>> {
        read_json_lines(&self.operations_file)
    }

    pub fn load_checkpoint(&self) -> Result<V3OpsCheckpoint> {
        read_json_file(&self.checkpoint_file)
    }

    pub fn load_observations_file(&self, name: &str) -> Result<Vec<V3ObservationEvent>> {
        read_json_lines(&self.observations_dir.join(name))
    }

    pub fn save_targets(&self, value: &V3TargetsFile) -> Result<()> {
        write_json_file(&self.targets_file, value)
    }

    pub fn save_bindings(&self, value: &V3BindingsFile) -> Result<()> {
        write_json_file(&self.bindings_file, value)
    }

    pub fn save_rules(&self, value: &V3RulesFile) -> Result<()> {
        write_json_file(&self.rules_file, value)
    }

    pub fn save_projections(&self, value: &V3ProjectionsFile) -> Result<()> {
        write_json_file(&self.projections_file, value)
    }

    pub fn save_checkpoint(&self, value: &V3OpsCheckpoint) -> Result<()> {
        write_json_file(&self.checkpoint_file, value)
    }

    pub fn append_operation(&self, value: &V3OperationRecord) -> Result<()> {
        append_json_line(&self.operations_file, value)
    }
}

impl V3Snapshot {
    pub fn status_view(&self) -> serde_json::Value {
        let unique_skills = self
            .rules
            .rules
            .iter()
            .map(|rule| rule.skill_id.as_str())
            .chain(
                self.projections
                    .projections
                    .iter()
                    .map(|p| p.skill_id.as_str()),
            )
            .collect::<std::collections::BTreeSet<_>>();

        let drifted = self
            .projections
            .projections
            .iter()
            .filter(|projection| {
                projection.observed_drift.unwrap_or(false) || projection.health != "healthy"
            })
            .count();

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
            "projections": self.projections.projections,
            "checkpoint": self.checkpoint
        })
    }

    pub fn binding(&self, binding_id: &str) -> Option<&V3WorkspaceBinding> {
        self.bindings
            .bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
    }

    pub fn target(&self, target_id: &str) -> Option<&V3ProjectionTarget> {
        self.targets
            .targets
            .iter()
            .find(|target| target.target_id == target_id)
    }

    pub fn binding_default_target(
        &self,
        binding: &V3WorkspaceBinding,
    ) -> Option<V3ProjectionTarget> {
        self.target(&binding.default_target_id).cloned()
    }

    pub fn binding_rules(&self, binding_id: &str) -> Vec<V3BindingRule> {
        self.rules
            .rules
            .iter()
            .filter(|rule| rule.binding_id == binding_id)
            .cloned()
            .collect()
    }

    pub fn binding_projections(&self, binding_id: &str) -> Vec<V3ProjectionInstance> {
        self.projections
            .projections
            .iter()
            .filter(|projection| projection.binding_id == binding_id)
            .cloned()
            .collect()
    }

    pub fn target_rules(&self, target_id: &str) -> Vec<V3BindingRule> {
        self.rules
            .rules
            .iter()
            .filter(|rule| rule.target_id == target_id)
            .cloned()
            .collect()
    }

    pub fn target_projections(&self, target_id: &str) -> Vec<V3ProjectionInstance> {
        self.projections
            .projections
            .iter()
            .filter(|projection| projection.target_id == target_id)
            .cloned()
            .collect()
    }

    pub fn target_bindings(&self, target_id: &str) -> Vec<V3WorkspaceBinding> {
        let rules = self.target_rules(target_id);
        let projections = self.target_projections(target_id);

        self.bindings
            .bindings
            .iter()
            .filter(|binding| {
                binding.default_target_id == target_id
                    || rules
                        .iter()
                        .any(|rule| rule.binding_id == binding.binding_id)
                    || projections
                        .iter()
                        .any(|projection| projection.binding_id == binding.binding_id)
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Snapshot {
    pub schema: V3SchemaFile,
    pub targets: V3TargetsFile,
    pub bindings: V3BindingsFile,
    pub rules: V3RulesFile,
    pub projections: V3ProjectionsFile,
    pub operations: Vec<V3OperationRecord>,
    pub checkpoint: V3OpsCheckpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3SchemaFile {
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub writer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3TargetsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub targets: Vec<V3ProjectionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3BindingsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub bindings: Vec<V3WorkspaceBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3RulesFile {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Vec<V3BindingRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ProjectionsFile {
    pub schema_version: u32,
    #[serde(default)]
    pub projections: Vec<V3ProjectionInstance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ProjectionTarget {
    pub target_id: String,
    pub agent: String,
    pub path: String,
    pub ownership: String,
    pub capabilities: V3TargetCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3TargetCapabilities {
    pub symlink: bool,
    pub copy: bool,
    pub watch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3WorkspaceBinding {
    pub binding_id: String,
    pub agent: String,
    pub profile_id: String,
    pub workspace_matcher: V3WorkspaceMatcher,
    pub default_target_id: String,
    pub policy_profile: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3WorkspaceMatcher {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3BindingRule {
    pub binding_id: String,
    pub skill_id: String,
    pub target_id: String,
    pub method: String,
    pub watch_policy: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ProjectionInstance {
    pub instance_id: String,
    pub skill_id: String,
    pub binding_id: String,
    pub target_id: String,
    pub materialized_path: String,
    pub method: String,
    pub last_applied_rev: String,
    pub health: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_drift: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3OperationRecord {
    pub op_id: String,
    pub intent: String,
    pub status: String,
    pub ack: bool,
    pub payload: serde_json::Value,
    pub effects: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<V3OperationError>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3OperationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3OpsCheckpoint {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_scanned_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_acked_op_id: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3ObservationEvent {
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

fn empty_targets_file() -> V3TargetsFile {
    V3TargetsFile {
        schema_version: V3_SCHEMA_VERSION,
        targets: Vec::new(),
    }
}

fn empty_bindings_file() -> V3BindingsFile {
    V3BindingsFile {
        schema_version: V3_SCHEMA_VERSION,
        bindings: Vec::new(),
    }
}

fn empty_rules_file() -> V3RulesFile {
    V3RulesFile {
        schema_version: V3_SCHEMA_VERSION,
        rules: Vec::new(),
    }
}

fn empty_projections_file() -> V3ProjectionsFile {
    V3ProjectionsFile {
        schema_version: V3_SCHEMA_VERSION,
        projections: Vec::new(),
    }
}

fn validate_schema_version(version: u32) -> Result<()> {
    if version != V3_SCHEMA_VERSION {
        return Err(anyhow!(
            "v3 schema version mismatch: expected {}, got {}",
            V3_SCHEMA_VERSION,
            version
        ));
    }
    Ok(())
}

fn ensure_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if path.exists() {
        return Ok(());
    }
    write_json_file(path, value)
}

fn ensure_text_file(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_atomic(path, contents)
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let raw = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to encode v3 json file {}", path.display()))?;
    write_atomic(path, &(raw + "\n"))
}

fn append_json_line<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .context("cannot append v3 jsonl file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    let raw = serde_json::to_string(value)
        .with_context(|| format!("failed to encode v3 jsonl line {}", path.display()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open v3 jsonl file {}", path.display()))?;
    writeln!(file, "{}", raw)
        .with_context(|| format!("failed to append v3 jsonl file {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync v3 jsonl file {}", path.display()))?;
    Ok(())
}

fn read_json_file<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read v3 json file {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse v3 json file {}", path.display()))
}

fn read_json_lines<T>(path: &Path) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open v3 jsonl file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| {
            format!("failed to read line {} from {}", index + 1, path.display())
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed to parse line {} from v3 jsonl file {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(item);
    }
    Ok(items)
}

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("cannot write atomic v3 file without parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directory {}", parent.display()))?;

    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));

    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file {}", tmp_path.display()))?;
    }

    fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp_path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        V3BindingRule, V3BindingsFile, V3OperationRecord, V3OpsCheckpoint, V3ProjectionInstance,
        V3ProjectionTarget, V3ProjectionsFile, V3SchemaFile, V3Snapshot, V3StatePaths,
        V3TargetCapabilities, V3TargetsFile, V3WorkspaceBinding, V3WorkspaceMatcher,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn builds_expected_v3_paths() {
        let paths = V3StatePaths::from_root(Path::new("/tmp/loom"));
        assert_eq!(paths.v3_dir, Path::new("/tmp/loom/state/v3"));
        assert_eq!(
            paths.operations_file,
            Path::new("/tmp/loom/state/v3/ops/operations.jsonl")
        );
        assert_eq!(
            paths.observations_dir,
            Path::new("/tmp/loom/state/v3/observations")
        );
    }

    #[test]
    fn query_helpers_link_bindings_targets_and_projections() {
        let now = Utc::now();
        let snapshot = V3Snapshot {
            schema: V3SchemaFile {
                schema_version: 3,
                created_at: now,
                writer: "loom-test".to_string(),
            },
            targets: V3TargetsFile {
                schema_version: 3,
                targets: vec![V3ProjectionTarget {
                    target_id: "target_claude".to_string(),
                    agent: "claude".to_string(),
                    path: "/tmp/claude/skills".to_string(),
                    ownership: "managed".to_string(),
                    capabilities: V3TargetCapabilities {
                        symlink: true,
                        copy: true,
                        watch: true,
                    },
                    created_at: Some(now),
                }],
            },
            bindings: V3BindingsFile {
                schema_version: 3,
                bindings: vec![V3WorkspaceBinding {
                    binding_id: "binding_project_a".to_string(),
                    agent: "claude".to_string(),
                    profile_id: "default".to_string(),
                    workspace_matcher: V3WorkspaceMatcher {
                        kind: "path_prefix".to_string(),
                        value: "/tmp/project-a".to_string(),
                    },
                    default_target_id: "target_claude".to_string(),
                    policy_profile: "safe-capture".to_string(),
                    active: true,
                    created_at: Some(now),
                }],
            },
            rules: super::V3RulesFile {
                schema_version: 3,
                rules: vec![V3BindingRule {
                    binding_id: "binding_project_a".to_string(),
                    skill_id: "model-onboarding".to_string(),
                    target_id: "target_claude".to_string(),
                    method: "symlink".to_string(),
                    watch_policy: "observe_only".to_string(),
                    created_at: Some(now),
                }],
            },
            projections: V3ProjectionsFile {
                schema_version: 3,
                projections: vec![V3ProjectionInstance {
                    instance_id: "instance_1".to_string(),
                    skill_id: "model-onboarding".to_string(),
                    binding_id: "binding_project_a".to_string(),
                    target_id: "target_claude".to_string(),
                    materialized_path: "/tmp/claude/skills/model-onboarding".to_string(),
                    method: "symlink".to_string(),
                    last_applied_rev: "abc123".to_string(),
                    health: "healthy".to_string(),
                    observed_drift: Some(false),
                    updated_at: Some(now),
                }],
            },
            operations: vec![V3OperationRecord {
                op_id: "op_001".to_string(),
                intent: "skill.project".to_string(),
                status: "succeeded".to_string(),
                ack: false,
                payload: json!({"skill_id": "model-onboarding"}),
                effects: json!({"instance_id": "instance_1"}),
                last_error: None,
                created_at: now,
                updated_at: now,
            }],
            checkpoint: V3OpsCheckpoint {
                schema_version: 3,
                last_scanned_op_id: Some("op_001".to_string()),
                last_acked_op_id: None,
                updated_at: now,
            },
        };

        assert!(snapshot.binding("binding_project_a").is_some());
        assert!(snapshot.target("target_claude").is_some());
        assert_eq!(snapshot.binding_rules("binding_project_a").len(), 1);
        assert_eq!(snapshot.binding_projections("binding_project_a").len(), 1);
        assert_eq!(snapshot.target_rules("target_claude").len(), 1);
        assert_eq!(snapshot.target_projections("target_claude").len(), 1);
        assert_eq!(snapshot.target_bindings("target_claude").len(), 1);
    }
}
